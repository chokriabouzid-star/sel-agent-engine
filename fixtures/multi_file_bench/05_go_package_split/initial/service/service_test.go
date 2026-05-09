package service

import "testing"
import "example.com/go_user_service/user"

func TestGreet(t *testing.T) {
    u := user.User{Name: "Alice"}
    got := Greet(u)
    if got != "Hello Alice" {
        t.Fatalf("unexpected %s", got)
    }
}
