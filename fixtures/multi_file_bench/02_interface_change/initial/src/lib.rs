pub mod service;

pub struct User {
    pub name: String,
}

pub fn user_name(u: &User) -> &str {
    &u.name
}
