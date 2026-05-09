from a import hello_a

def test_circular():
    assert "A -> B" in hello_a()
