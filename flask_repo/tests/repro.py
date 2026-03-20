import pytest
from flask import Flask

def test_issue_1234():
    app = Flask(__name__)
    with app.test_client() as c:
        resp = c.get('/')
        assert resp.status_code == 200
        assert b'Hello' in resp.data
