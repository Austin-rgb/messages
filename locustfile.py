from locust import HttpUser, task, between
import uuid
import random
import json
import time

import requests

# -------------------------
# CONFIGURATION
# -------------------------

BASE_HEADERS = {
    "Content-Type": "application/json",
}

def login(username):
	auth = requests.post('http://localhost:8000',json={'username':username,'password':'password123'}).json()
	return auth['data']['access_token']

# Replace with real JWTs or dynamically generate them
# Each token should represent a different user (sub)
TOKENS = [
    f"Bearer {login('alice')}",
    f"Bearer {login('bob')}",
    f"Bearer {login('charlie')}",
]

REACTIONS = [1, 2, 3, 4, 5]


class ChatUser(HttpUser):
    wait_time = between(1, 3)

    def on_start(self):
        """
        Called once per simulated user
        """
        self.token = random.choice(TOKENS)
        self.headers = {
            **BASE_HEADERS,
            "Authorization": self.token,
        }

        self.conversation_name = None
        self.message_ids = []

        self.create_conversation()

    # -------------------------
    # HELPERS
    # -------------------------

    def create_conversation(self):
        payload = {
            "title": f"Load Test Conversation {uuid.uuid4()}",
            "participants": ["user1", "user2", "user3"],
        }

        with self.client.post(
            "/conversations",
            headers=self.headers,
            json=payload,
            catch_response=True,
        ) as response:
            if response.status_code == 200:
                data = response.json()
                self.conversation_name = data["name"]
                response.success()
            else:
                response.failure(f"Failed to create conversation: {response.text}")

    # -------------------------
    # TASKS
    # -------------------------

    @task(3)
    def list_conversations(self):
        self.client.get(
            "/conversations",
            headers=self.headers,
            name="GET /conversations",
        )

    @task(2)
    def get_conversation(self):
        if not self.conversation_name:
            return

        self.client.get(
            f"/conversations/{self.conversation_name}",
            headers=self.headers,
            name="GET /conversations/{name}",
        )

    @task(4)
    def post_message(self):
        if not self.conversation_name:
            return

        payload = {
            "text": f"Hello from Locust at {time.time()}",
            "reply_to": None,
        }

        with self.client.post(
            f"/conversations/{self.conversation_name}/messages",
            headers=self.headers,
            json=payload,
            name="POST /conversations/{name}/messages",
            catch_response=True,
        ) as response:
            if response.status_code == 200:
                msg = response.json()
                self.message_ids.append(msg["id"])
                response.success()
            else:
                response.failure("Failed to post message")

    @task(3)
    def get_messages(self):
        if not self.conversation_name:
            return

        self.client.get(
            f"/conversations/{self.conversation_name}/messages?limit=20&offset=0",
            headers=self.headers,
            name="GET /conversations/{name}/messages",
        )

    @task(1)
    def react_to_message(self):
        if not self.message_ids:
            return

        msg_id = random.choice(self.message_ids)
        reaction = random.choice(REACTIONS)

        self.client.get(
            f"/messages/{msg_id}/react/{reaction}",
            headers=self.headers,
            name="GET /messages/{msg}/react/{reaction}",
        )

    @task(1)
    def mark_message_as_read(self):
        if not self.message_ids:
            return

        msg_id = random.choice(self.message_ids)

        self.client.get(
            f"/messages/{msg_id}/mark_as_read",
            headers=self.headers,
            name="GET /messages/{msg}/mark_as_read",
        )

    @task(1)
    def get_receipts(self):
        if not self.message_ids:
            return

        msg_id = random.choice(self.message_ids)

        self.client.get(
            f"/messages/{msg_id}/receipts",
            headers=self.headers,
            name="GET /messages/{msg}/receipts",
        )
