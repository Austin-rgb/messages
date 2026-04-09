#!/usr/bin/env python3
import json
import os
import sys
from typing import Any, Dict, Optional

import requests

BASE_URL = os.getenv("BASE_URL", "http://127.0.0.1:8082")
USERNAME = os.getenv("USERNAME", "alice")
PEER = os.getenv("PEER", "bob")
AUTH_HEADER = os.getenv("AUTH_HEADER", "Authorization")
AUTH_VALUE = os.getenv("AUTH_VALUE", "")  # e.g. "Bearer <token>"


def auth_headers() -> Dict[str, str]:
    if not AUTH_VALUE:
        return {}
    return {AUTH_HEADER: AUTH_VALUE}


def request_json(method: str, path: str, payload: Optional[Dict[str, Any]] = None) -> requests.Response:
    url = f"{BASE_URL}{path}"
    headers = {"Content-Type": "application/json"}
    headers.update(auth_headers())
    resp = requests.request(method, url, headers=headers, data=json.dumps(payload) if payload is not None else None)
    return resp


def assert_ok(resp: requests.Response, label: str) -> Any:
    try:
        body = resp.json() if resp.content else None
    except Exception:
        body = resp.text
    if resp.status_code >= 400:
        print(f"[FAIL] {label}: {resp.status_code} {body}")
        sys.exit(1)
    print(f"[OK] {label}: {resp.status_code}")
    return body


def main() -> None:
    # 1) Create conversation
    create_payload = {
        "title": "Test Conversation",
        "participants": [PEER],
    }
    resp = request_json("POST", "/conversations", create_payload)
    conv = assert_ok(resp, "create_conversation")
    conv_name = conv.get("name") if isinstance(conv, dict) else None
    if not conv_name:
        print("[FAIL] create_conversation: missing conversation name")
        sys.exit(1)

    # 2) List conversations
    resp = request_json("GET", "/conversations")
    assert_ok(resp, "list_conversations")

    # 3) Get conversation
    resp = request_json("GET", f"/conversations/{conv_name}")
    assert_ok(resp, "get_conversation")

    # 4) Post message to conversation
    msg_payload = {"text": "hello from test", "reply_to": None}
    resp = request_json("POST", f"/conversations/{conv_name}/messages", msg_payload)
    assert_ok(resp, "post_message")

    # 5) Get messages from conversation
    resp = request_json("GET", f"/conversations/{conv_name}/messages")
    msgs = assert_ok(resp, "get_messages")
    msg_id = None
    if isinstance(msgs, list) and msgs:
        msg_id = msgs[0].get("id")

    # 6) Send peer message (direct inbox)
    resp = request_json("POST", f"/inbox/{PEER}/messages", msg_payload)
    assert_ok(resp, "peer_message")

    # 7) Get inbox messages
    resp = request_json("GET", "/inbox/messages")
    inbox = assert_ok(resp, "get_pmessages")
    if not msg_id and isinstance(inbox, list) and inbox:
        msg_id = inbox[0].get("id")

    # 8) Get receipts (only if we have a message id)
    if msg_id:
        resp = request_json("GET", f"/messages/{msg_id}/receipts")
        assert_ok(resp, "get_receipts")

        # 9) React
        resp = request_json("GET", f"/messages/{msg_id}/react/1")
        assert_ok(resp, "react")

        # 10) Mark as read
        resp = request_json("GET", f"/messages/{msg_id}/mark_as_read")
        assert_ok(resp, "mark_as_read")

    print("All handler tests completed successfully.")


if __name__ == "__main__":
    main()
