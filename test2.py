import requests
import websocket
from time import sleep, time
import uuid
from json import loads


AUTH_BASE = "http://localhost:8000/api/auth"
MSG_BASE = "http://127.0.0.1:8080"
WS_URL = "ws://127.0.0.1:8080/ws/"
CONCURRENT_CONNECTIONS = 12
# -----------------------------
# Helpers
# -----------------------------

from threading import Thread


# timer utility
class Timer:
    def __init__(self) -> None:
        self.start = time()

    @property
    def value(self) -> float:
        return time() - self.start


class WorkPool:
    def __init__(self, nworkers, handler, work: list = []) -> None:
        self.handler = handler
        self.nworkers = nworkers
        self.proceed = False
        self._outputs = [None for i in work]
        self.threads: list[Thread] = []
        self.work: list = work

    def worker_handler(self):
        while self.proceed and len(self.work) > 0:
            i = len(self.work)
            self._outputs[i - 1] = self.handler(*self.work.pop())

    def resume(self):
        if not self.proceed:
            self.threads = [
                Thread(target=self.worker_handler) for i in range(self.nworkers)
            ]
            self.proceed = True
            for th in self.threads:
                th.start()

    def start(self):
        self.resume()
        return self

    @property
    def output(self):
        return self._outputs

    def pause(self):

        self.proceed = False
        for th in self.threads:
            th.join()

    def wait(self):
        for th in self.threads:
            th.join()


def user(name):
    return {"username": f"{name}_{uuid.uuid4().hex[:6]}", "password": "password123"}


def register(session, u):
    session.post(f"{AUTH_BASE}/register", json=u)


def login(session, u):
    with session.post(f"{AUTH_BASE}/login", json=u) as r:
        data = r.json()
        return data["data"]["access_token"]


def create_conversation(session, token, participants):
    with session.post(
        f"{MSG_BASE}/conversations",
        json={"participants": participants},
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return (r.json())["name"]


def send_message(session, token, conv, text):
    session.post(
        f"{MSG_BASE}/conversations/{conv}/messages",
        json={"text": text},
        headers={"Authorization": f"Bearer {token}"},
    )


def fetch_messages(session, token, conv):
    with session.get(
        f"{MSG_BASE}/conversations/{conv}/messages",
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return r.json()


def ws_client(name, token, handler):
    def on_message(ws, message):
        handler(loads(message))
        print(time(), message)

    def on_connect(ws):
        print("connected", name)

    ws = websocket.WebSocketApp(
        WS_URL,
        header={"Authorization": f"Bearer {token}"},
        on_message=on_message,
        on_open=on_connect,
    )
    ws.run_forever()


# -----------------------------
# Test
# -----------------------------


if __name__ == "__main__":
    session = requests.Session()

    names = ["alice", "bob", "carol", "diana", "elvis", "felix"]
    users = [user(name) for name in names]
    print("🔐 Registering users...")
    work = [(session, u) for u in users]
    wp = WorkPool(10, register, work)
    wp.start()
    wp.wait()

    print("🔑 Logging in...")
    work = [(session, u) for u in users]
    wp = WorkPool(len(work), login, work)
    wp.start().wait()
    tokens: list[str] = wp.output
    inboxes: list[list] = [[] for i in users]

    print("📡 Connecting WebSockets...")

    WorkPool(
        len(users),
        ws_client,
        [(u, t, i.append) for u, t, i in zip(users, tokens, inboxes)],
    ).start()

    # -----------------------------
    # Peer-to-peer
    # -----------------------------
    print("💬 Testing P2P conversation...")
    conv_p2p = create_conversation(session, tokens[0], [users[1]["username"]])

    send_message(session, tokens[0], conv_p2p, "P2P hello")
    sleep(2)
    print(inboxes)
    history = fetch_messages(session, tokens[0], conv_p2p)
    # assert any("P2P hello" in m["text"] for m in history)
    print(inboxes[1])
    assert any("P2P hello" in m["text"] for m in inboxes[1])

    print("✅ P2P OK")

    # -----------------------------
    # Group chat
    # -----------------------------
    print("👥 Testing group chat...")
    conv_group = create_conversation(session, tokens[0], [u["username"] for u in users])

    send_message(session, tokens[1], conv_group, "Hello group")
    sleep(1)
    print(inboxes)
    for inbox in inboxes:
        if inbox != inboxes[1]:
            assert any("Hello group" in m["text"] for m in inbox)

    history = fetch_messages(session, tokens[-1], conv_group)
    # assert any("Hello group" in m["text"] for m in history)

    print("✅ Group chat OK")

    print("Testing load capacity")

    msgs = 0

    def func():
        global msgs
        send_message(session, tokens[0], conv_group, f"Hello for the th time")
        msgs += 1

    timer = Timer()
    thds = []
    wp = WorkPool(CONCURRENT_CONNECTIONS, func, [() for i in range(900)]).start()
    sleep(15)
    wp.pause()
    print(
        f"took {timer.value} seconds to send {msgs} messages: {msgs/timer.value} req/sec"
    )
    print(
        f"received {len(inboxes[1])} messages in {timer.value} seconds: {len(inboxes[1])/timer.value} msgs/sec"
    )
    history = fetch_messages(session, tokens[-1], conv_group)
    print(f"written {len(history)} messages in {timer.value} seconds")
    print("\n🎉 ALL MESSAGE TESTS PASSED")
