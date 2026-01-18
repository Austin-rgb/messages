import requests


class LoginFailed(Exception):
    pass


class User:
    def __init__(self, username, password="password123") -> None:
        res = requests.post(
            "http://localhost:8000/auth/login",
            json={"username": username, "password": password},
            timeout=10,
        )
        if res.ok:
            res = res.json()
            self.username = username
            self.access = res["data"]["access_token"]
            self.refresh = res["data"]["refresh_token"]

        else:
            print("[login] status:", res.status_code, res.content)
            raise LoginFailed

    @staticmethod
    def register(username, password="password123") -> bool:
        res = requests.post(
            "http://localhost:8000/auth/register",
            json={"username": username, "password": password},
            timeout=10,
        )
        return res.ok

    def __repr__(self) -> str:
        return f"User({self.username})"
