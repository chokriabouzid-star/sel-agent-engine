class Flask:
    def __init__(self, name):
        self.name = name
        self.routes = {}
    
    def route(self, path):
        def decorator(func):
            self.routes[path] = func
            return func
        return decorator

def create_app():
    app = Flask(__name__)
    
    @app.route('/')
    def hello():
        return 'Hello, World!'
    
    return app