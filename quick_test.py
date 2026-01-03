#!/usr/bin/env python3
"""
Quick test script for the Chat API
"""

import requests
import json
import sys

class QuickTester:
    def __init__(self):
        self.auth_url = "http://localhost:8000"
        self.api_url = "http://localhost:8080"
        self.tokens = {}
    
    def login(self, username="alice", password="password123"):
        """Quick login helper"""
        try:
            response = requests.post(
                f"{self.auth_url}/api/auth/login",
                json={"username": username, "password": password}
            )
            if response.status_code == 200:
                self.tokens[username] = response.json()["data"]['access_token']
                print(f"✅ Logged in as {username}")
                return True
            else:
                print(f"❌ Login failed for {username}: {response.text}")
                return False
        except Exception as e:
            print(f"❌ Connection error: {e}")
            return False
    
    def test_basic_flow(self):
        """Test basic conversation flow"""
        print("\n🔍 Testing basic flow...")
        
        # Login as alice
        if not self.login("alice"):
            return
        
        # Create conversation
        headers = {
            "Authorization": f"Bearer {self.tokens['alice']}",
            "Content-Type": "application/json"
        }
        
        conv_data = {
            "participants": ["bob"],
            "title": "Quick Test Chat"
        }
        
        response = requests.post(
            f"{self.api_url}/conversations",
            json=conv_data,
            headers=headers
        )
        
        if response.status_code == 200:
            conversation = response.json()
            conv_name = conversation['name']
            print(f"✅ Created conversation: {conv_name}")
            
            # Post a message
            msg_data = {"text": "Hello Bob!"}
            response = requests.post(
                f"{self.api_url}/conversations/{conv_name}/messages",
                json=msg_data,
                headers=headers
            )
            
            if response.status_code == 200:
                print(f"✅ Posted message: {response.json()['text']}")
            else:
                print(f"❌ Failed to post message: {response.text}")
            
            # Get messages
            response = requests.get(
                f"{self.api_url}/conversations/{conv_name}/messages",
                headers=headers,
                params={"limit": 10}
            )
            
            if response.status_code == 200:
                messages = response.json()
                print(f"✅ Retrieved {len(messages)} messages")
            else:
                print(f"❌ Failed to get messages: {response.text}")
            
            # List conversations
            response = requests.get(
                f"{self.api_url}/conversations",
                headers=headers
            )
            
            if response.status_code == 200:
                conversations = response.json()
                print(f"✅ Alice has {len(conversations)} conversation(s)")
            else:
                print(f"❌ Failed to list conversations: {response.text}")
                
        else:
            print(f"❌ Failed to create conversation: {response.text}")
    
    def check_services(self):
        """Check if services are running"""
        print("🔍 Checking services...")
        
        try:
            # Check auth service
            response = requests.get(f"{self.auth_url}/health", timeout=2)
            print(f"✅ Auth service: {response.status_code}")
        except:
            print("❌ Auth service not reachable")
        
        try:
            # Check chat API
            response = requests.get(f"{self.api_url}/health", timeout=2)
            print(f"✅ Chat API: {response.status_code}")
        except:
            print("❌ Chat API not reachable")
            print("Note: Add a /health endpoint to your Rust app for this check")

if __name__ == "__main__":
    tester = QuickTester()
    
    if len(sys.argv) > 1 and sys.argv[1] == "check":
        tester.check_services()
    else:
        tester.test_basic_flow()
