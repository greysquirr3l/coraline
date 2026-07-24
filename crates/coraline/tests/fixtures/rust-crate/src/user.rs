//! User management

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
}

pub struct UserService {
    users: HashMap<u64, User>,
    next_id: u64,
}

impl UserService {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn create_user(&mut self, name: String, email: String) -> User {
        let user = User {
            id: self.next_id,
            name,
            email,
        };
        self.next_id += 1;
        self.users.insert(user.id, user.clone());
        user
    }

    pub fn get_user(&self, id: u64) -> Option<&User> {
        self.users.get(&id)
    }

    pub fn update_user(&mut self, id: u64, name: Option<String>, email: Option<String>) -> Option<User> {
        if let Some(user) = self.users.get_mut(&id) {
            if let Some(n) = name {
                user.name = n;
            }
            if let Some(e) = email {
                user.email = e;
            }
            Some(user.clone())
        } else {
            None
        }
    }

    pub fn delete_user(&mut self, id: u64) -> bool {
        self.users.remove(&id).is_some()
    }

    pub fn get_all_users(&self) -> Vec<&User> {
        self.users.values().collect()
    }
}

impl Default for UserService {
    fn default() -> Self {
        Self::new()
    }
}
