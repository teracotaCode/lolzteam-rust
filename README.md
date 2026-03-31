# lolzteam

[![CI](https://github.com/teracotaCode/lolzteam-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/teracotaCode/lolzteam-rust/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Rust-обёртка для API Lolzteam Forum и Market.

## Возможности

- **Клиенты Forum и Market** с типизированными моделями запросов/ответов
- **Автоматическое ограничение частоты запросов** по алгоритму token-bucket
- **Повторные попытки с экспоненциальной задержкой** и джиттером при временных ошибках
- **Поддержка прокси** (HTTP, HTTPS, SOCKS5)
- **Async/await** на базе `tokio` и `reqwest`

## Установка

Добавьте в ваш `Cargo.toml`:

```toml
[dependencies]
# Клонируйте репозиторий и подключите как path dependency
# git clone https://github.com/teracotaCode/lolzteam-rust.git
lolzteam = { path = "../lolzteam-rust" }
tokio = { version = "1", features = ["full"] }
```

## Быстрый старт

```rust,no_run
use lolzteam::ForumClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ForumClient::new("ваш-api-токен")?;
    // Используйте client.service().method(...).await?
    Ok(())
}
```

```rust,no_run
use lolzteam::MarketClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = MarketClient::new("ваш-api-токен")?;
    // Используйте client.service().method(...).await?
    Ok(())
}
```

## Конфигурация

```rust,no_run
use lolzteam::{ForumClient, ClientConfig, ProxyConfig};
use std::time::Duration;

let config = ClientConfig {
    proxy: Some(ProxyConfig {
        url: "socks5://127.0.0.1:1080".into(),
    }),
    timeout: Duration::from_secs(30),  // таймаут запроса
    max_retries: 5,                    // максимальное число повторных попыток
    ..ClientConfig::forum("ваш-токен")
};

let client = ForumClient::with_config(config).unwrap();
```

## Разработка

```bash
# Сборка
make build

# Тесты

# Линтер (fmt + clippy)
make lint

# Перегенерация API-кода из схем
make generate
```

## Лицензия

MIT
