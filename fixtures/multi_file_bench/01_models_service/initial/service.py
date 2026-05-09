from models import User

def greet(user: User) -> str:
    return f"Hello {user.name}"
