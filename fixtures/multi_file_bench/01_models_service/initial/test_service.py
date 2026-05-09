from service import greet
from models import User

def test_greet():
    assert greet(User("Alice")) == "Hello Alice"
