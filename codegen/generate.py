#!/usr/bin/env python3
"""Code generator for Lolzteam API Rust wrapper.

Reads an OpenAPI 3 JSON schema and produces:
  - types.rs  (response structs, param structs, enums)
  - client.rs (service struct with API group accessors and async methods)

Usage:
  python3 codegen/generate.py --schema schemas/forum.json --output-dir src/generated/forum --module-name forum
  python3 codegen/generate.py --schema schemas/market.json --output-dir src/generated/market --module-name market
"""

import argparse
import json
import os
import re
import sys
from collections import OrderedDict

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

RUST_RESERVED = {
    "as", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
    "super", "trait", "true", "type", "unsafe", "use", "where", "while",
    "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
    "try",
}

# Large enums (game name catalogues etc.) are collapsed to String.
MAX_ENUM_VARIANTS = 50

# ---------------------------------------------------------------------------
# Name helpers
# ---------------------------------------------------------------------------

def safe_rust_ident(name: str) -> str:
    """Make *name* a legal Rust identifier."""
    if not name:
        return "unknown"
    if name in RUST_RESERVED:
        return name + "_"
    if name[0].isdigit():
        return "n_" + name
    return name


def sanitize_to_ident(s: str) -> str:
    """Strip everything that isn't [a-zA-Z0-9_] and collapse runs."""
    s = re.sub(r'[^a-zA-Z0-9_]', '_', s)
    s = re.sub(r'_+', '_', s).strip('_')
    return s


def to_pascal_case(s: str) -> str:
    s = s.replace(".", "_")
    # Replace any non-ident chars with underscore first
    s = sanitize_to_ident(s)
    parts = s.split('_')
    result = []
    for p in parts:
        if not p:
            continue
        # Split camelCase
        sub = re.sub(r'([a-z])([A-Z])', r'\1_\2', p)
        for sp in sub.split('_'):
            if sp:
                result.append(sp[0].upper() + sp[1:])
    return ''.join(result)


def to_snake_case(s: str) -> str:
    s = sanitize_to_ident(s)
    s = re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', s)
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', s)
    s = s.lower()
    s = re.sub(r'_+', '_', s).strip('_')
    return s


def sanitize_variant(s: str) -> str:
    """Turn an arbitrary string into a valid PascalCase Rust variant name."""
    v = to_pascal_case(s)
    if not v:
        return "Empty"
    if v[0].isdigit():
        v = 'V' + v
    return v


# ---------------------------------------------------------------------------
# Schema resolver
# ---------------------------------------------------------------------------

class SchemaResolver:
    def __init__(self, spec: dict):
        self.spec = spec
        self._resolving: set = set()

    def resolve(self, schema: dict) -> dict:
        if not isinstance(schema, dict):
            return schema
        ref = schema.get("$ref")
        if ref:
            if ref in self._resolving:
                return {"type": "object"}
            self._resolving.add(ref)
            resolved = self._follow_ref(ref)
            resolved = self.resolve(resolved)
            self._resolving.discard(ref)
            merged = dict(resolved)
            for k, v in schema.items():
                if k != "$ref":
                    merged[k] = v
            return merged
        out = {}
        for k, v in schema.items():
            if k == "properties" and isinstance(v, dict):
                out[k] = {pk: self.resolve(pv) for pk, pv in v.items()}
            elif k == "items" and isinstance(v, dict):
                out[k] = self.resolve(v)
            elif k in ("additionalProperties",) and isinstance(v, dict):
                out[k] = self.resolve(v)
            elif k in ("oneOf", "allOf", "anyOf") and isinstance(v, list):
                out[k] = [self.resolve(i) for i in v]
            else:
                out[k] = v
        return out

    def _follow_ref(self, ref: str) -> dict:
        assert ref.startswith("#/"), f"External refs unsupported: {ref}"
        parts = ref[2:].split("/")
        node = self.spec
        for p in parts:
            node = node[p]
        return dict(node)


# ---------------------------------------------------------------------------
# Code generator
# ---------------------------------------------------------------------------

class CodeGenerator:
    def __init__(self, spec: dict, module_name: str):
        self.spec = spec
        self.module_name = module_name
        self.resolver = SchemaResolver(spec)
        self.structs: OrderedDict = OrderedDict()   # name -> [(json_name, rust_field, rust_type, needs_rename)]
        self.enums: OrderedDict = OrderedDict()     # name -> [(json_val, rust_variant)]
        self.extra_code: list = []                   # type aliases etc.
        self._used_names: set = set()
        self._needs_string_or_int = False
        self.groups: OrderedDict = OrderedDict()    # group_key -> [EndpointInfo]

    # -- name allocation --
    def _alloc_name(self, desired: str) -> str:
        name = desired
        i = 2
        while name in self._used_names:
            name = f"{desired}{i}"
            i += 1
        self._used_names.add(name)
        return name

    # -- helpers --
    @staticmethod
    def _is_dynamic_dict(props: dict) -> bool:
        """Return True when ALL property keys are purely numeric, OR when
        any numeric key is mixed with non-numeric keys (indicating the
        schema was generated from example data with ID keys).

        This means the schema is not a real struct but a dynamic dict
        keyed by IDs.
        """
        if not props:
            return False
        has_numeric = any(k.isdigit() for k in props)
        return has_numeric

    # -- schema -> Rust type --
    def schema_to_rust_type(self, schema: dict, parent_name: str, field_name: str = "") -> str:
        if not schema:
            return "serde_json::Value"
        schema = self.resolver.resolve(schema)

        # allOf: merge properties
        if "allOf" in schema:
            merged = {}
            for sub in schema["allOf"]:
                sub = self.resolver.resolve(sub)
                for k, v in sub.items():
                    if k == "properties":
                        merged.setdefault("properties", {}).update(v)
                    elif k == "required":
                        merged.setdefault("required", []).extend(v)
                    else:
                        merged[k] = v
            schema = merged

        if "oneOf" in schema and "properties" not in schema:
            return "serde_json::Value"
        if "anyOf" in schema and "properties" not in schema:
            return "serde_json::Value"

        schema_type = schema.get("type")

        # Multi-type ["string", "integer"]
        if isinstance(schema_type, list):
            type_set = set(schema_type) - {"null"}
            if type_set <= {"string", "integer"} or type_set <= {"string", "number"}:
                self._needs_string_or_int = True
                return "StringOrInt"
            if len(type_set) == 1:
                schema_type = type_set.pop()
            else:
                return "serde_json::Value"

        # String enum
        if "enum" in schema and schema_type == "string":
            values = schema["enum"]
            if len(values) > MAX_ENUM_VARIANTS:
                return "String"
            return self._register_enum(parent_name, field_name, values)

        # Integer enum -> i64
        if "enum" in schema and schema_type == "integer":
            return "i64"

        # Scalars
        if schema_type == "string":
            if schema.get("format") == "binary":
                return "Vec<u8>"
            return "String"
        if schema_type == "integer":
            return "i64"
        if schema_type == "number":
            return "f64"
        if schema_type == "boolean":
            return "bool"

        # Array
        if schema_type == "array":
            items = schema.get("items", {})
            if not items:
                return "Vec<serde_json::Value>"
            # If array items are a dynamic-dict object, the whole field
            # should be flexible (PHP APIs serialise numeric-keyed arrays
            # as either JSON objects or JSON arrays unpredictably).
            items_resolved = self.resolver.resolve(items)
            items_props = items_resolved.get("properties", {})
            if self._is_dynamic_dict(items_props):
                return "serde_json::Value"
            inner = self.schema_to_rust_type(items, parent_name, field_name)
            return f"Vec<{inner}>"

        # Object with properties -> struct
        if schema_type == "object" or "properties" in schema:
            props = schema.get("properties", {})
            if not props:
                add_props = schema.get("additionalProperties")
                if isinstance(add_props, dict) and add_props:
                    inner = self.schema_to_rust_type(add_props, parent_name, field_name)
                    return f"std::collections::HashMap<String, {inner}>"
                return "serde_json::Value"
            # Dynamic dict detection: all-numeric keys mean this is
            # example data from a map keyed by IDs, not a real struct.
            if self._is_dynamic_dict(props):
                return "serde_json::Value"
            struct_name = self._alloc_name(
                f"{parent_name}{to_pascal_case(field_name)}" if field_name else parent_name
            )
            self._register_struct(struct_name, props)
            return struct_name

        return "serde_json::Value"

    def _register_enum(self, parent: str, field: str, values: list) -> str:
        name = f"{parent}{to_pascal_case(field)}" if field else parent
        if name in self.enums:
            return name
        name = self._alloc_name(name)
        variants = []
        seen = set()
        for v in values:
            vr = sanitize_variant(str(v))
            if vr == "Unknown":
                vr = "UnknownValue"
            orig = vr
            c = 2
            while vr in seen:
                vr = f"{orig}{c}"
                c += 1
            seen.add(vr)
            variants.append((str(v), vr))
        self.enums[name] = variants
        return name

    # API type mismatch overrides — real API returns different types than spec
    FIELD_TYPE_OVERRIDES = {
        "priceWithSellerFee": "f64",
        "roblox_credit_balance": "f64",
        "steam_bans": "serde_json::Value",
        "guarantee": "serde_json::Value",
        "cs2PremierElo": "serde_json::Value",
        "discord_nitro_type": "serde_json::Value",
        "instagram_id": "serde_json::Value",
        "socialclub_games": "serde_json::Value",
        "feedback_data": "serde_json::Value",
        "imap_data": "serde_json::Value",
        "restore_data": "serde_json::Value",
        "telegram_client": "serde_json::Value",
        "backgrounds": "serde_json::Value",
        "steam_full_games": "serde_json::Value",
        "thread_tags": "serde_json::Value",
        "Skin": "serde_json::Value",
        "WeaponSkins": "serde_json::Value",
        "supercellBrawlers": "serde_json::Value",
        "r6Skins": "serde_json::Value",
        "tags": "serde_json::Value",
        "values": "serde_json::Value",
        "base_params": "serde_json::Value",
        # API returns content_id as string in some contexts, int in others
        "content_id": "StringOrInt",
        # PHP API returns false instead of null/int for these fields
        "autoBuyPrice": "serde_json::Value",
        "autoBuyPriceCheckDate": "serde_json::Value",
        "aiPrice": "serde_json::Value",
        "aiPriceCheckDate": "serde_json::Value",
        "ai_price": "serde_json::Value",
        "ai_price_check_date": "serde_json::Value",
        "auto_buy_price": "serde_json::Value",
        "auto_buy_price_check_date": "serde_json::Value",
    }

    def _register_struct(self, name: str, properties: dict):
        if name in self.structs:
            return
        self.structs[name] = []  # reserve against recursion
        fields = []
        for json_name, prop_schema in properties.items():
            prop_schema = self.resolver.resolve(prop_schema)
            rf = to_snake_case(json_name)
            rf = safe_rust_ident(rf)
            if json_name in self.FIELD_TYPE_OVERRIDES:
                rt = self.FIELD_TYPE_OVERRIDES[json_name]
            else:
                rt = self.schema_to_rust_type(prop_schema, name, json_name)
            needs_rename = (rf != json_name)
            fields.append((json_name, rf, rt, needs_rename))
        self.structs[name] = fields

    # -- component schemas --
    def process_component_schemas(self):
        schemas = self.spec.get("components", {}).get("schemas", {})
        for name, schema in schemas.items():
            schema = self.resolver.resolve(schema)
            rust_name = to_pascal_case(name)
            schema_type = schema.get("type")

            if isinstance(schema_type, list):
                type_set = set(schema_type) - {"null"}
                if type_set <= {"string", "integer"} or type_set <= {"string", "number"}:
                    self._needs_string_or_int = True
                    self.extra_code.append(f"pub type {rust_name} = StringOrInt;")
                    self._used_names.add(rust_name)
                    continue

            if schema_type == "integer" and "properties" not in schema and "enum" not in schema:
                self.extra_code.append(f"pub type {rust_name} = i64;")
                self._used_names.add(rust_name)
                continue

            if schema_type == "string" and "enum" in schema:
                values = schema["enum"]
                if len(values) > MAX_ENUM_VARIANTS:
                    self.extra_code.append(f"pub type {rust_name} = String;")
                    self._used_names.add(rust_name)
                    continue
                enum_name = self._alloc_name(rust_name)
                variants = []
                seen = set()
                for v in values:
                    vr = sanitize_variant(str(v))
                    if vr == "Unknown":
                        vr = "UnknownValue"
                    orig = vr
                    c = 2
                    while vr in seen:
                        vr = f"{orig}{c}"
                        c += 1
                    seen.add(vr)
                    variants.append((str(v), vr))
                self.enums[enum_name] = variants
                continue

            if schema_type == "object" or "properties" in schema:
                props = schema.get("properties", {})
                if props:
                    # Dynamic dict detection for component schemas
                    if self._is_dynamic_dict(props):
                        self.extra_code.append(f"pub type {rust_name} = serde_json::Value;")
                        self._used_names.add(rust_name)
                    else:
                        sname = self._alloc_name(rust_name)
                        self._register_struct(sname, props)
                continue

    # -- endpoints --
    def process_endpoints(self):
        paths = self.spec.get("paths", {})
        for path, path_item in paths.items():
            for http_method, op in path_item.items():
                if http_method not in ("get", "post", "put", "delete", "patch"):
                    continue
                self._process_endpoint(path, http_method, op)

    def _process_endpoint(self, path: str, http_method: str, op: dict):
        operation_id = op.get("operationId", "")
        tags = op.get("tags", ["Default"])
        tag = tags[0] if tags else "Default"

        if "." in operation_id:
            parts = operation_id.split(".")
            group_name = parts[0]
            method_name = "_".join(parts[1:])
        else:
            group_name = to_pascal_case(tag)
            method_name = operation_id or "unknown"

        group_key = to_pascal_case(group_name)
        method_snake = to_snake_case(method_name)
        if not method_snake:
            method_snake = "call"
        method_snake = safe_rust_ident(method_snake)

        # Path params / query params
        path_params = []
        query_params = []
        for param in op.get("parameters", []):
            param = self.resolver.resolve(param)
            param_schema = self.resolver.resolve(param.get("schema", {}))
            p_in = param.get("in", "")
            p_name = param.get("name", "")
            if not p_name:
                continue
            if p_in == "path":
                path_params.append((p_name, to_snake_case(p_name), self._scalar_type(param_schema)))
            elif p_in == "query":
                query_params.append((p_name, param_schema))

        # Response type
        response_is_text = self._is_text_html_response(op)
        resp_schema, resp_shared_name = self._response_schema_with_name(op)
        base_resp = f"{group_key}{to_pascal_case(method_snake)}Response"
        if response_is_text:
            resp_type = "String"
        elif resp_schema and (resp_schema.get("properties") or resp_schema.get("$ref") or resp_schema.get("allOf")):
            if resp_shared_name:
                # Shared response from components/responses — reuse/create one struct
                resp_type = self._ensure_shared_response(resp_shared_name, resp_schema)
            else:
                resp_type_name = self._alloc_name(base_resp)
                resp_type = self.schema_to_rust_type(resp_schema, resp_type_name)
        else:
            # No response schema found or empty schema — generate a minimal response struct
            resp_type_name = self._alloc_name(base_resp)
            self._register_struct(resp_type_name, {
                "status": {"type": "string"},
                "system_info": {"type": "object"},
            })
            resp_type = resp_type_name

        # Params struct
        params_struct = None
        if query_params:
            params_struct = self._alloc_name(f"{group_key}{to_pascal_case(method_snake)}Params")
            fields = []
            for qn, qs in query_params:
                rf = safe_rust_ident(to_snake_case(qn))
                rt = self.schema_to_rust_type(qs, params_struct, qn)
                fields.append((qn, rf, rt, rf != qn))
            self.structs[params_struct] = fields

        # Body struct
        body_struct = None
        is_multipart = False
        body = op.get("requestBody", {})
        if body:
            content = body.get("content", {})
            ct_key, body_schema = None, None
            for ct in ["application/json", "multipart/form-data"]:
                if ct in content:
                    ct_key = ct
                    body_schema = content[ct].get("schema", {})
                    break
            if not ct_key and content:
                ct_key = list(content.keys())[0]
                body_schema = content[ct_key].get("schema", {})
            is_array_body = False
            if body_schema:
                body_schema = self.resolver.resolve(body_schema)
                is_multipart = ct_key == "multipart/form-data"
                # Check if body schema is an array (e.g. batch endpoints)
                if body_schema.get("type") == "array":
                    is_array_body = True
                # Flatten oneOf bodies
                elif "oneOf" in body_schema:
                    merged = {}
                    for v in body_schema["oneOf"]:
                        v = self.resolver.resolve(v)
                        for pk, pv in v.get("properties", {}).items():
                            if pk not in merged:
                                merged[pk] = pv
                    if merged:
                        body_schema = {"type": "object", "properties": merged}
                props = body_schema.get("properties", {})
                if not is_array_body and props:
                    body_struct = self._alloc_name(f"{group_key}{to_pascal_case(method_snake)}Body")
                    fields = []
                    for bn, bs in props.items():
                        bs = self.resolver.resolve(bs)
                        rf = safe_rust_ident(to_snake_case(bn))
                        rt = self.schema_to_rust_type(bs, body_struct, bn)
                        fields.append((bn, rf, rt, rf != bn))
                    self.structs[body_struct] = fields

        is_search = "search" in operation_id.lower() or "/search" in path.lower()

        # For multipart endpoints, classify body fields as binary vs text
        multipart_binary_fields = []  # [(json_name, rust_field_name)]
        multipart_text_fields = []    # [(json_name, rust_field_name, rust_type)]
        if is_multipart and body_struct and body_struct in self.structs:
            for jn, rf, rt, _rn in self.structs[body_struct]:
                if rt == "Vec<u8>":
                    multipart_binary_fields.append((jn, rf))
                else:
                    multipart_text_fields.append((jn, rf, rt))

        ep = EndpointInfo(
            path=path,
            http_method=http_method.upper(),
            method_name=method_snake,
            path_params=path_params,
            params_struct=params_struct,
            body_struct=body_struct,
            response_type=resp_type,
            is_search=is_search,
            is_multipart=is_multipart,
            operation_id=operation_id,
            multipart_binary_fields=multipart_binary_fields,
            multipart_text_fields=multipart_text_fields,
            is_array_body=is_array_body if body else False,
            response_is_text=response_is_text,
        )
        self.groups.setdefault(group_key, []).append(ep)

    def _scalar_type(self, schema: dict) -> str:
        t = schema.get("type", "string")
        if isinstance(t, list):
            return "i64" if "integer" in t else "String"
        return {"integer": "i64", "number": "f64"}.get(t, "String")

    def _response_schema(self, op: dict) -> dict:
        schema, _ = self._response_schema_with_name(op)
        return schema

    def _is_text_html_response(self, op: dict) -> bool:
        """Check if the operation's 200 response is text/html (not JSON)."""
        for code in ("200",):
            resp = op.get("responses", {}).get(code, {})
            if "$ref" in resp:
                resp = self._resolve_response_ref(resp["$ref"])
                if not resp:
                    continue
            ct = resp.get("content", {})
            if "text/html" in ct and "application/json" not in ct:
                return True
        return False

    def _response_schema_with_name(self, op: dict) -> tuple:
        """Returns (resolved_schema, shared_name_or_None).
        shared_name is set when the response comes from a $ref to components/responses."""
        for code in ("200", "201", "202", "204"):
            resp = op.get("responses", {}).get(code, {})
            # Resolve response-level $ref (e.g. "$ref": "#/components/responses/SaveChanges")
            shared_name = None
            if "$ref" in resp:
                shared_name = self._ref_leaf_name(resp["$ref"])
                resp = self._resolve_response_ref(resp["$ref"])
                if not resp:
                    continue
            ct = resp.get("content", {})
            if "application/json" in ct:
                schema = self.resolver.resolve(ct["application/json"].get("schema", {}))
                return schema, shared_name
        return {}, None

    def _ref_leaf_name(self, ref: str) -> str:
        """Extract the last component of a $ref path."""
        return ref.rsplit("/", 1)[-1] if "/" in ref else ref

    def _resolve_response_ref(self, ref: str) -> dict:
        """Resolve a $ref pointing to #/components/responses/..."""
        if not ref.startswith("#/"):
            return {}
        parts = ref[2:].split("/")
        node = self.spec
        for p in parts:
            if isinstance(node, dict) and p in node:
                node = node[p]
            else:
                return {}
        return node if isinstance(node, dict) else {}

    def _ensure_shared_response(self, name: str, schema: dict) -> str:
        """Create (or reuse) a struct for a shared component response."""
        rust_name = to_pascal_case(name) + "Response"
        if rust_name in self.structs:
            return rust_name
        # Register it directly: reserve the name and build the struct
        # without going through schema_to_rust_type (which would call _alloc_name
        # and potentially create a different name).
        schema = self.resolver.resolve(schema)
        # Merge allOf if needed
        if "allOf" in schema:
            merged = {}
            for sub in schema["allOf"]:
                sub = self.resolver.resolve(sub)
                for k, v in sub.items():
                    if k == "properties":
                        merged.setdefault("properties", {}).update(v)
                    elif k == "required":
                        merged.setdefault("required", []).extend(v)
                    else:
                        merged[k] = v
            schema = merged
        props = schema.get("properties", {})
        if props:
            self._used_names.add(rust_name)
            self._register_struct(rust_name, props)
        else:
            # Fallback: no properties, just create a minimal struct
            self._used_names.add(rust_name)
            self._register_struct(rust_name, {
                "status": {"type": "string"},
                "system_info": {"type": "object"},
            })
        return rust_name

    # -----------------------------------------------------------------------
    # Emit types.rs
    # -----------------------------------------------------------------------
    def emit_types_rs(self) -> str:
        L = []
        L.append("// Auto-generated by codegen/generate.py \u2014 DO NOT EDIT")
        L.append("#![allow(non_camel_case_types, unused_imports)]")
        L.append("")
        L.append("use serde::{Deserialize, Serialize};")
        if self._needs_string_or_int:
            L.append("use crate::runtime::types::StringOrInt;")
        L.append("")

        # Enums
        for ename, variants in self.enums.items():
            L.append("#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]")
            L.append(f"pub enum {ename} {{")
            for jv, rv in variants:
                L.append(f'    #[serde(rename = "{_escape(jv)}")]')
                L.append(f"    {rv},")
            L.append("    #[serde(other)]")
            L.append("    Unknown,")
            L.append("}")
            L.append("")
            L.append(f"impl Default for {ename} {{")
            L.append(f"    fn default() -> Self {{ {ename}::Unknown }}")
            L.append("}")
            L.append("")

        # Extra code (type aliases)
        for c in self.extra_code:
            L.append(c)
            L.append("")

        # Structs
        for sname, fields in self.structs.items():
            L.append("#[derive(Debug, Clone, Default, Serialize, Deserialize)]")
            L.append("#[serde(default)]")
            L.append(f"pub struct {sname} {{")
            seen: set = set()
            for jn, rf, rt, needs_rename in fields:
                # Deduplicate field names
                orig_rf = rf
                c = 2
                while rf in seen:
                    rf = f"{orig_rf}_{c}"
                    c += 1
                    needs_rename = True
                seen.add(rf)
                if needs_rename:
                    L.append(f'    #[serde(rename = "{_escape(jn)}")]')
                L.append("    #[serde(default)]")
                L.append(f"    pub {rf}: Option<{rt}>,")
            L.append("}")
            L.append("")

        return "\n".join(L)

    # -----------------------------------------------------------------------
    # Emit client.rs
    # -----------------------------------------------------------------------
    def emit_client_rs(self) -> str:
        svc = f"{to_pascal_case(self.module_name)}Service"
        L = []
        L.append("// Auto-generated by codegen/generate.py \u2014 DO NOT EDIT")
        L.append("")
        L.append("use std::sync::Arc;")
        L.append("use crate::runtime::HttpClient;")
        L.append("use crate::runtime::types::RequestOptions;")
        L.append("use crate::runtime::errors::LolzteamError;")
        L.append("use super::types::*;")
        L.append("")

        # Service
        L.append(f"/// Generated {to_pascal_case(self.module_name)} API service.")
        L.append("#[derive(Debug, Clone)]")
        L.append(f"pub struct {svc} {{")
        L.append("    pub(crate) http: Arc<HttpClient>,")
        L.append("}")
        L.append("")
        L.append(f"impl {svc} {{")
        L.append(f"    pub fn new(http: Arc<HttpClient>) -> Self {{ Self {{ http }} }}")
        L.append("")
        for gk in self.groups:
            acc = safe_rust_ident(to_snake_case(gk))
            api = f"{gk}Api"
            L.append(f"    pub fn {acc}(&self) -> {api}<'_> {{")
            L.append(f"        {api} {{ http: &self.http }}")
            L.append("    }")
            L.append("")
        L.append("}")
        L.append("")

        # API groups
        for gk, eps in self.groups.items():
            api = f"{gk}Api"
            L.append(f"pub struct {api}<'a> {{")
            L.append("    http: &'a HttpClient,")
            L.append("}")
            L.append("")
            L.append(f"impl<'a> {api}<'a> {{")
            used: set = set()
            for ep in eps:
                mn = ep.method_name
                base = mn
                c = 2
                while mn in used:
                    mn = f"{base}_{c}"
                    c += 1
                used.add(mn)
                L.append(self._emit_method(ep, mn))
                L.append("")
            L.append("}")
            L.append("")

        return "\n".join(L)

    def _emit_method(self, ep, mn: str) -> str:
        L = []
        args = ["&self"]
        # For array body schemas (e.g. batch), add a jobs parameter
        if ep.is_array_body:
            args.append("jobs: &[serde_json::Value]")
        for _, rn, rt in ep.path_params:
            rn = safe_rust_ident(rn)
            args.append(f"{rn}: {rt}" if rt in ("i64", "f64", "bool") else f"{rn}: &str")
        if ep.params_struct:
            args.append(f"params: Option<&{ep.params_struct}>")
        if not ep.is_array_body and ep.body_struct:
            args.append(f"body: Option<&{ep.body_struct}>")

        sig = ", ".join(args)
        L.append(f"    pub async fn {mn}({sig}) -> Result<{ep.response_type}, LolzteamError> {{")

        # Path interpolation
        path_str = ep.path
        for jn, rn, _ in ep.path_params:
            path_str = path_str.replace("{" + jn + "}", "{" + safe_rust_ident(rn) + "}")
        if ep.path_params:
            fmt_args = ", ".join(f"{safe_rust_ident(rn)} = {safe_rust_ident(rn)}" for _, rn, _ in ep.path_params)
            L.append(f'        let path = format!("{path_str}", {fmt_args});')
        else:
            L.append(f'        let path = "{path_str}".to_string();')

        # Query params
        if ep.params_struct:
            L.append("        let query = params.map(|p| {")
            L.append("            let val = serde_json::to_value(p).unwrap_or_default();")
            L.append("            let mut pairs: Vec<(String, String)> = Vec::new();")
            L.append("            if let serde_json::Value::Object(map) = val {")
            L.append("                for (k, v) in map {")
            L.append("                    match v {")
            L.append("                        serde_json::Value::Null => {}")
            L.append("                        serde_json::Value::String(s) => pairs.push((k, s)),")
            L.append('                        serde_json::Value::Bool(b) => pairs.push((k, if b { "1".to_string() } else { "0".to_string() })),')
            L.append("                        serde_json::Value::Number(n) => pairs.push((k, n.to_string())),")
            L.append("                        serde_json::Value::Array(arr) => {")
            L.append('                            let ks = format!("{}[]", k);')
            L.append("                            for item in arr {")
            L.append("                                match item {")
            L.append("                                    serde_json::Value::String(s) => pairs.push((ks.clone(), s)),")
            L.append("                                    other => pairs.push((ks.clone(), other.to_string())),")
            L.append("                                }")
            L.append("                            }")
            L.append("                        }")
            L.append("                        other => pairs.push((k, other.to_string())),")
            L.append("                    }")
            L.append("                }")
            L.append("            }")
            L.append("            pairs")
            L.append("        });")
        else:
            L.append("        let query: Option<Vec<(String, String)>> = None;")

        # Body handling: multipart vs JSON
        is_search = "true" if ep.is_search else "false"
        if ep.is_multipart and ep.body_struct:
            # Multipart: split body fields into file uploads and text fields
            has_binary = len(ep.multipart_binary_fields) > 0
            has_text = len(ep.multipart_text_fields) > 0
            files_mut = "mut " if has_binary else ""
            fields_mut = "mut " if has_text else ""
            L.append(f"        let {files_mut}files: Vec<crate::runtime::types::FileUpload> = Vec::new();")
            L.append(f"        let {fields_mut}multipart_fields: Vec<(String, String)> = Vec::new();")
            L.append("        if let Some(b) = body {")
            for jn, rf in ep.multipart_binary_fields:
                L.append(f"            if let Some(ref data) = b.{rf} {{")
                L.append(f"                files.push(crate::runtime::types::FileUpload {{")
                L.append(f'                    field_name: "{_escape(jn)}".to_string(),')
                L.append(f'                    file_name: "{_escape(jn)}".to_string(),')
                L.append(f'                    mime_type: "application/octet-stream".to_string(),')
                L.append(f"                    data: data.clone(),")
                L.append(f"                }});")
                L.append(f"            }}")
            for jn, rf, rt in ep.multipart_text_fields:
                L.append(f"            if let Some(ref val) = b.{rf} {{")
                # Primitive types can use to_string(); complex types need serde
                if rt in ("String", "i64", "f64", "bool"):
                    L.append(f'                multipart_fields.push(("{_escape(jn)}".to_string(), val.to_string()));')
                else:
                    L.append(f'                multipart_fields.push(("{_escape(jn)}".to_string(), serde_json::to_string(val).unwrap_or_default()));')
                L.append(f"            }}")
            L.append("        }")
            L.append("        let opts = RequestOptions {")
            L.append("            query,")
            L.append("            json: None,")
            L.append("            form: None,")
            L.append("            files: if files.is_empty() { None } else { Some(files) },")
            L.append("            multipart_fields: if multipart_fields.is_empty() { None } else { Some(multipart_fields) },")
            L.append(f"            is_search: {is_search},")
            L.append("        };")
        elif ep.is_array_body:
            # Array body (e.g. batch endpoints) — send the array as JSON body directly
            L.append("        let json_body = Some(serde_json::Value::Array(jobs.to_vec()));")
            L.append("        let opts = RequestOptions {")
            L.append("            query,")
            L.append("            json: json_body,")
            L.append("            form: None,")
            L.append("            files: None,")
            L.append("            multipart_fields: None,")
            L.append(f"            is_search: {is_search},")
            L.append("        };")
        else:
            # Standard JSON body
            if ep.body_struct:
                L.append("        let json_body = body.map(|b| serde_json::to_value(b).unwrap_or_default());")
            else:
                L.append("        let json_body: Option<serde_json::Value> = None;")
            L.append("        let opts = RequestOptions {")
            L.append("            query,")
            L.append("            json: json_body,")
            L.append("            form: None,")
            L.append("            files: None,")
            L.append("            multipart_fields: None,")
            L.append(f"            is_search: {is_search},")
            L.append("        };")

        L.append(f'        let resp = self.http.request("{ep.http_method}", &path, opts).await?;')
        if ep.response_is_text:
            # Text response — extract raw string
            L.append('        match resp {')
            L.append('            serde_json::Value::String(s) => Ok(s),')
            L.append('            other => Ok(other.to_string()),')
            L.append('        }')
        else:
            L.append("        serde_json::from_value(resp).map_err(|e| LolzteamError::Config(e.to_string()))")
        L.append("    }")
        return "\n".join(L)


def _escape(s: str) -> str:
    """Escape a string for use inside a Rust string literal."""
    return s.replace("\\", "\\\\").replace('"', '\\"')


class EndpointInfo:
    __slots__ = ('path', 'http_method', 'method_name', 'path_params',
                 'params_struct', 'body_struct', 'response_type',
                 'is_search', 'is_multipart', 'operation_id',
                 'multipart_binary_fields', 'multipart_text_fields',
                 'is_array_body', 'response_is_text')
    def __init__(self, **kw):
        for k, v in kw.items():
            setattr(self, k, v)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--schema", required=True)
    ap.add_argument("--output-dir", required=True)
    ap.add_argument("--module-name", required=True)
    args = ap.parse_args()

    with open(args.schema) as f:
        spec = json.load(f)

    gen = CodeGenerator(spec, args.module_name)
    gen.process_component_schemas()
    gen.process_endpoints()

    os.makedirs(args.output_dir, exist_ok=True)
    for fname, content in [("types.rs", gen.emit_types_rs()),
                            ("client.rs", gen.emit_client_rs())]:
        p = os.path.join(args.output_dir, fname)
        with open(p, "w") as f:
            f.write(content)
        print(f"  {p}")
    print(f"  {len(gen.structs)} structs, {len(gen.enums)} enums, "
          f"{len(gen.groups)} groups, {sum(len(v) for v in gen.groups.values())} methods")


if __name__ == "__main__":
    main()
