import requests
from time import time


AUTH_BASE = "http://localhost:8000/api/auth"
MSG_BASE = "http://127.0.0.1:8080"
WS_URL = "ws://127.0.0.1:8080/ws/"


# timer utility
class Timer:
    def __init__(self) -> None:
        self.start = time()

    @property
    def value(self) -> float:
        return time() - self.start

login = requests.post(AUTH_BASE+'/login',json={'username':'alice','password':'password123'}).json()
token = login['data']['access_token']

timer = Timer()
for i in range(1000):

    requests.post(
        MSG_BASE + "/conversations",
        json={
            "name": "failed",
            "participants": ["me", "you"],
            "title": "my failed title",
        },
        timeout=10,
        headers={"AUTHORIZATION": f"Bearer {token}"},
    )


print(f"took {timer.value} seconds: {1000/timer.value} req/sec")
