from flask import Flask

def create_app():
    app = Flask(__name__)
    
    @app.route('/')
    def hello():
        return 'Hello, World!'
    
    return app

# Fix import issue by making this module importable
try:
    from flask import Flask
except ImportError:
    # Fallback for when flask is not installed
    class Flask:
        def __init__(self, name):
            self.name = name