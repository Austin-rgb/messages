# Real-Time Chat Service (Actix + WebSocket + Redis Streams)

A scalable real-time messaging backend built with **Rust**, **Actix-web**, **WebSockets**, **Redis Streams**, and **SQLite**.
Supports multi-user conversations, message delivery, receipts, reactions, and background workers for persistence.

---

## Features

- Multi-participant conversations
- Authenticated REST + WebSocket API
- Real-time message fan-out via Actix actors
- Redis Streams for durable event buffering
- Message receipts (delivered / read / reaction)
- Background workers for async DB persistence
- Participant caching for fast authorization

---

## Architecture Overview

### Core components

- **HTTP API (Actix)** — conversation & message management
- **WebSocket Server (Actix actors)** — live message delivery
- **Redis Streams** — message & receipt event bus
- **SQLite (SQLx)** — persistent storage
- **Workers** — consume Redis streams and persist events

### Flow (message send)

1. Client sends message via REST
2. Message pushed to `messages_stream` (Redis)
3. Message delivered live via WebSocket actor
4. Background worker persists message to SQLite
5. Receipts are generated and pushed to `receipts_stream`

---

## Database Schema

### `conversations`

| Column  | Type | Notes           |
| ------- | ---- | --------------- |
| name    | TEXT | Conversation ID |
| title   | TEXT | Optional title  |
| admin   | TEXT | Creator / owner |
| created | INT  | Timestamp       |

### `participants`

| Column       | Type | Notes                    |
| ------------ | ---- | ------------------------ |
| conversation | TEXT | FK → conversations(name) |
| participant  | TEXT | Username                 |

### `messages`

| Column       | Type | Notes                    |
| ------------ | ---- | ------------------------ |
| id           | TEXT | Message UUID             |
| conversation | TEXT | FK → conversations(name) |
| source       | TEXT | Sender                   |
| text         | TEXT | Message body             |
| reply_to     | TEXT | Optional parent message  |
| created      | INT  | Timestamp                |

### `message_receipts`

| Column     | Type | Notes                  |
| ---------- | ---- | ---------------------- |
| message_id | TEXT | FK → messages(id)      |
| user_id    | TEXT | Receiver               |
| delivered  | BOOL | Delivered flag         |
| read       | BOOL | Read flag              |
| reaction   | INT  | Optional reaction code |
| ts         | INT  | Event timestamp        |

---

## Redis Streams

| Stream            | Purpose                     |
| ----------------- | --------------------------- |
| `messages_stream` | Buffer outgoing messages    |
| `receipts_stream` | Buffer delivery/read events |

Workers consume these streams and write to SQLite.

---

## Endpoints

All endpoints require authentication via `Auth` middleware.

### Conversations

#### `POST /conversations`

Create a new conversation.

Body:

- `participants: [string]`
- `title?: string`
- `name?: string`

---

#### `GET /conversations`

List all conversations the user participates in.

---

#### `GET /conversations/{name}`

Get conversation metadata.

---

### Messages

#### `POST /conversations/{name}/messages`

Send a message to a conversation.

Body:

- `text: string`
- `reply_to?: string`

Behavior:

- Validates participation
- Pushes event to Redis
- Delivers live to connected users

---

#### `GET /conversations/{name}/messages`

Retrieve message history.

Query filters supported via `MessageFilters`:

- pagination
- ordering
- limits

Also generates automatic delivery receipts.

---

### Receipts & Reactions

#### `GET /messages/{msg}/receipts`

Get delivery/read receipts for a message (sender only).

---

#### `GET /messages/{msg}/react/{reaction}`

Send a reaction event for a message.

---

#### `GET /messages/{msg}/mark_as_read`

Mark a message as read.

---

## WebSocket

Endpoint:

```
/ws
```

- Authenticated via middleware
- Users subscribe automatically to their conversations
- Messages are pushed in real-time via Actix actor system

---

## Background Workers

### `db_worker`

- Consumes `messages_stream`
- Persists messages into SQLite

### `receipt_worker`

- Consumes `receipts_stream`
- Persists delivery, read, and reaction events

---

## Running the Service

### Requirements

- Rust (stable)
- Redis
- SQLite

### Environment

Create `.env` file or export variables:

```bash
# JWT / auth handled by auth_middleware
# Redis config loaded via redis_cfg
```

### Run

```bash
cargo run
```

Server starts on:

```
http://127.0.0.1:8082
```

Database file:

```
messages.db
```

Redis must be running before startup.

---

## Concurrency & Guarantees

- Message delivery is non-blocking
- Persistence is eventually consistent
- Live delivery does not wait for DB writes
- Redis provides backpressure and durability

---

## Intended Use

Suitable as:

- Messaging backend prototype
- WebSocket + Redis Streams reference
- Event-driven system example in Rust
- Chat system foundation for web or mobile apps

---

## Status

Advanced prototype.
Missing production features:

- Message deletion / editing
- Typing indicators
- Pagination indexes tuning
- Conversation ACLs
- Backfill / retention policies

---

## License

Not specified yet.
