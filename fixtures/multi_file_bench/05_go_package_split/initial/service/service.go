package service

import "example.com/go_user_service/user"

func Greet(u user.User) string {
    return "Hello " + u.Name
}
