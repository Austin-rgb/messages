import asyncio
import aiohttp
import websockets
import json
import uuid
from time import time


AUTH_BASE = "http://localhost:8000/api/auth"
MSG_BASE = "http://127.0.0.1:8080"
WS_URL = "ws://127.0.0.1:8080/ws/"

# -----------------------------
# Helpers
# -----------------------------


# timer utility
class Timer:
    def __init__(self) -> None:
        self.start = time()

    @property
    def value(self) -> float:
        return time() - self.start


def user(name):
    return {"username": f"{name}_{uuid.uuid4().hex[:6]}", "password": "password123"}


async def register(session, u):
    await session.post(f"{AUTH_BASE}/register", json=u)


async def login(session, u):
    async with session.post(f"{AUTH_BASE}/login", json=u) as r:
        data = await r.json()
        return data["data"]["access_token"]


async def create_conversation(session, token, participants):
    async with session.post(
        f"{MSG_BASE}/conversations",
        json={"participants": participants},
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return (await r.json())["name"]


async def send_message(session, token, conv, text):
    await session.post(
        f"{MSG_BASE}/conversations/{conv}/messages",
        json={"text": text},
        headers={"Authorization": f"Bearer {token}"},
    )


async def fetch_messages(session, token, conv):
    async with session.get(
        f"{MSG_BASE}/conversations/{conv}/messages",
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return await r.json()


async def ws_client(name, token, inbox):
    headers = [("Authorization", f"Bearer {token}")]
    async with websockets.connect(WS_URL, additional_headers=headers) as ws:
        print(f"[WS] {name} connected")
        while True:
            msg = await ws.recv()
            inbox.append(json.loads(msg))


# -----------------------------
# Test
# -----------------------------


async def main():
    async with aiohttp.ClientSession() as session:

        print("🔐 Registering users...")
        A = user("alice")
        B = user("bob")
        C = user("carol")

        for u in (A, B, C):
            await register(session, u)

        print("🔑 Logging in...")
        tokA = await login(session, A)
        tokB = await login(session, B)
        tokC = await login(session, C)

        inboxA, inboxB, inboxC = [], [], []

        print("📡 Connecting WebSockets...")
        ws_tasks = [
            asyncio.create_task(ws_client(A["username"], tokA, inboxA)),
            asyncio.create_task(ws_client(B["username"], tokB, inboxB)),
            asyncio.create_task(ws_client(C["username"], tokC, inboxC)),
        ]

        await asyncio.sleep(1)

        # -----------------------------
        # Peer-to-peer
        # -----------------------------
        print("💬 Testing P2P conversation...")
        conv_p2p = await create_conversation(
            session, tokA, [A["username"], B["username"]]
        )

        await send_message(session, tokA, conv_p2p, "P2P hello")
        await asyncio.sleep(1)

        assert any("P2P hello" in m["text"] for m in inboxB)
        history = await fetch_messages(session, tokA, conv_p2p)
        assert any("P2P hello" in m["text"] for m in history)

        print("✅ P2P OK")

        # -----------------------------
        # Group chat
        # -----------------------------
        print("👥 Testing group chat...")
        conv_group = await create_conversation(
            session, tokA, [A["username"], B["username"], C["username"]]
        )

        await send_message(session, tokB, conv_group, "Hello group")
        await asyncio.sleep(1)

        for inbox in (inboxA, inboxC):
            assert any("Hello group" in m["text"] for m in inbox)

        history = await fetch_messages(session, tokA, conv_group)
        assert any("Hello group" in m["text"] for m in history)

        print("✅ Group chat OK")

        print("Testing load capacity")
        timer = Timer()
        for i in range(200):
            await send_message(session, tokA, conv_group, f"Hello for the {i}th time")

        print(
            f"took {timer.value} seconds to send 1000 messages: {1000/timer.value} req/sec"
        )
        print(
            f"received {len(inboxB)} messages in {timer.value} seconds: {len(inboxB)/timer.value} msgs/sec"
        )
        print("\n🎉 ALL MESSAGE TESTS PASSED")

        for t in ws_tasks:
            t.cancel()


asyncio.run(main())
