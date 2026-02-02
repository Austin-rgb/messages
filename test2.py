import requests
import websocket
from time import sleep, time
import uuid
from json import loads
from auth import User


AUTH_BASE = "http://localhost/auth/auth"
MSG_BASE = "http://127.0.0.1/messages"
WS_URL = "ws://127.0.0.1/messages/ws/"
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
        self._outputs: list[tuple] = [None for i in work]  # type: ignore
        self.threads: list[Thread] = []
        self.work: list = work

    def worker_handler(self):
        while self.proceed and len(self.work) > 0:
            i = len(self.work)
            inp = self.work.pop()
            self._outputs[i - 1] = (inp, self.handler(*inp))

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


def send_pmessage(session, token, peer, text):
    session.post(
        f"{MSG_BASE}/inbox/{peer}/messages",
        json={"text": text},
        headers={"Authorization": f"Bearer {token}"},
    )


def fetch_messages(session, token, conv=None, peer=None):
    if conv:
        with session.get(
            f"{MSG_BASE}/conversations/{conv}/messages",
            headers={"Authorization": f"Bearer {token}"},
        ) as r:
            return r.json()

    if peer:
        with session.get(
            f"{MSG_BASE}/inbox/messages",
            headers={"Authorization": f"Bearer {token}"},
            params={"source": peer},
        ) as r:
            return r.json()


def fetch_pmessages(session, token, conv):
    with session.get(
        f"{MSG_BASE}//{conv}/messages",
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return r.json()


def fetch_receipts(session: requests.Session, token, message):
    with session.get(
        f"{MSG_BASE}/messages/{message}/receipts",
        headers={"Authorization": f"Bearer {token}"},
    ) as r:
        return r.json()


def ws_client(user: User, handler):
    def on_message(ws, message):
        print(message)
        handler(loads(message))

    def on_connect(ws):
        print("connected", user.username)

    ws = websocket.WebSocketApp(
        WS_URL,
        header={"Authorization": f"Bearer {user.access}"},
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
    names = [f"{name}_{uuid.uuid4().hex[:6]}" for name in names]
    users: list[str] = [user(name) for name in names]  # type: ignore
    print("🔐 Registering users...")
    work = [(u,) for u in names]
    wp = WorkPool(10, User.register, work)
    wp.start()
    wp.wait()

    work = [inp for inp, out in wp.output]
    print("🔑 Logging in...")
    wp = WorkPool(len(work), User, work)
    wp.start().wait()
    users: list[User] = [out for inp, out in wp.output]
    inboxes: list[list] = [[] for i in users]

    print("📡 Connecting WebSockets...")
    usrs = [u for u in users]
    WorkPool(
        len(users),
        ws_client,
        [(u, t.append) for u, t in zip(usrs, inboxes)],
    ).start()
    print(users)

    # -----------------------------
    # Peer-to-peer
    # -----------------------------
    print("💬 Testing P2P conversation...")
    # conv_p2p = create_conversation(session, users[0].access, [users[1].username])
    send_pmessage(session, users[0].access, users[1].username, "P2P hello")
    sleep(2)
    print(inboxes)
    history = fetch_messages(session, users[0].access, peer=users[1].username)
    print(inboxes[1])
    assert any("P2P hello" in m["text"] for m in inboxes[1])
    print("✅ P2P OK")
    print("Testing message receipts")
    receipts = fetch_receipts(session, users[0].access, history[0]["id"])
    print(receipts)

    # -----------------------------
    # Group chat
    # -----------------------------
    print("👥 Testing group chat...")
    conv_group = create_conversation(
        session, users[0].access, [u.username for u in users]
    )

    send_message(session, users[1].access, conv_group, "Hello group")
    sleep(1)
    print(inboxes)
    for inbox in inboxes:
        if inbox != inboxes[1]:
            assert any("Hello group" in m["text"] for m in inbox)

    history = fetch_messages(session, users[-1].access, conv=conv_group)
    # assert any("Hello group" in m["text"] for m in history)

    print("✅ Group chat OK")

    print("Testing load capacity")

    msgs = 0

    def func():
        global msgs
        send_message(session, users[0].access, conv_group, f"Hello for the th time")
        msgs += 1

    timer = Timer()
    thds = []
    wp = WorkPool(CONCURRENT_CONNECTIONS, func, [() for i in range(900)]).start()
    wp.wait()

    print(
        f"took {timer.value} seconds to send {msgs} messages: {msgs/timer.value} req/sec"
    )
    print(
        f"received {len(inboxes[1])} messages in {timer.value} seconds: {len(inboxes[1])/timer.value} msgs/sec"
    )
    sleep(2)
    history = fetch_messages(session, users[-1].access, conv_group)
    print(f"written {len(history)} messages in {timer.value} seconds")
    print("\n🎉 ALL MESSAGE TESTS PASSED")
