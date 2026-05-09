use crate::User;

pub fn greet(u: &User) -> String {
    format!("Hello {}", u.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::User;

    #[test]
    fn test_greet() {
        let u = User { name: "Alice".into() };
        assert_eq!(greet(&u), "Hello Alice");
    }
}
