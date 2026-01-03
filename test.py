import asyncio
import json
import time
import uuid
import requests
import websockets

# =========================
# CONFIG
# =========================

AUTH_BASE = "http://localhost:8000/api/auth"
MSG_BASE = "http://127.0.0.1:8080"
WS_URL = "ws://127.0.0.1:8080/ws/"

HEADERS_JSON = {"Content-Type": "application/json"}

# =========================
# AUTH HELPERS
# =========================

def register_user(username, password):
    payload = {
        "username": username,
        "password": password
    }
    r = requests.post(
        f"{AUTH_BASE}/register",
        headers=HEADERS_JSON,
        json=payload
    )
    if r.status_code not in (200, 201, 409):
        raise RuntimeError(f"Register failed: {r.text}")

def login_user(username, password):
    payload = {
        "username": username,
        "password": password
    }
    r = requests.post(
        f"{AUTH_BASE}/login",
        headers=HEADERS_JSON,
        json=payload
    )
    r.raise_for_status()
    return r.json()["data"]["access_token"]

# =========================
# REST HELPERS
# =========================

def auth_headers(token):
    return {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json"
    }

def create_conversation(token, participants, title="Test Chat"):
    payload = {
        "title": title,
        "participants": participants
    }
    r = requests.post(
        f"{MSG_BASE}/conversations",
        headers=auth_headers(token),
        json=payload
    )
    r.raise_for_status()
    return r.json()["name"]

def post_message(token, conversation, text):
    payload = {"text": text}
    r = requests.post(
        f"{MSG_BASE}/conversations/{conversation}/messages",
        headers=auth_headers(token),
        json=payload
    )
    r.raise_for_status()
    return r.json()

def get_messages(token, conversation):
    r = requests.get(
        f"{MSG_BASE}/conversations/{conversation}/messages",
        headers=auth_headers(token)
    )
    r.raise_for_status()
    return r.json()

# =========================
# WEBSOCKET CLIENT
# =========================

async def ws_client(user, token, inbox):
    headers = {
        "Authorization": f"Bearer {token}"
    }

    async with websockets.connect(WS_URL, additional_headers=headers) as ws:
        print(f"[WS] {user} connected")

        async def receiver():
            async for msg in ws:
                data = json.loads(msg)
                print(f"[WS] {user} received:", data)
                inbox.append(data)

        await receiver()

# =========================
# TEST SCENARIO
# =========================

async def main():
    # Unique users per run
    user_a = f"user_a_{uuid.uuid4().hex[:6]}"
    user_b = f"user_b_{uuid.uuid4().hex[:6]}"
    password = "password123"

    print("🔐 Registering users...")
    register_user(user_a, password)
    register_user(user_b, password)

    print("🔑 Logging in...")
    token_a = login_user(user_a, password)
    token_b = login_user(user_b, password)

    print("💬 Creating conversation...")
    conversation = create_conversation(
        token_a,
        participants=[user_b],
        title="WS Test"
    )

    inbox_a = []
    inbox_b = []

    print("📡 Connecting WebSockets...")
    ws_task_a = asyncio.create_task(ws_client(user_a, token_a, inbox_a))
    ws_task_b = asyncio.create_task(ws_client(user_b, token_b, inbox_b))

    await asyncio.sleep(1)

    print("✉️ Sending REST → WS message...")
    post_message(token_a, conversation, "Hello from REST")

    await asyncio.sleep(1)

    assert any("Hello from REST" in m["text"] for m in inbox_b), \
        "❌ REST → WS delivery failed"

    print("✉️ Sending WS → REST message...")
    ws_payload = {
        "type": "private",
        "to": user_a,
        "content": "Hello from WS"
    }

    async with websockets.connect(
        WS_URL,
        additional_headers={"Authorization": f"Bearer {token_b}"}
    ) as ws:
        await ws.send(json.dumps(ws_payload))
        await asyncio.sleep(1)

    messages = get_messages(token_a, conversation)
    assert any("Hello from WS" in m["text"] for m in messages), \
        "❌ WS → REST persistence failed"

    print("✅ All tests passed")

    ws_task_a.cancel()
    ws_task_b.cancel()

# =========================
# ENTRYPOINT
# =========================

if __name__ == "__main__":
    asyncio.run(main())
