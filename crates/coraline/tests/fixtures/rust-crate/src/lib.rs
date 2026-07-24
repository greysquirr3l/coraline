//! Simple test crate

pub mod math;
pub mod user;

use math::Calculator;
use user::UserService;

/// Application struct
pub struct App {
    calculator: Calculator,
    user_service: UserService,
}

impl App {
    pub fn new() -> Self {
        Self {
            calculator: Calculator::new(),
            user_service: UserService::new(),
        }
    }

    pub fn run(&mut self) {
        println!("Starting app...");
        
        // Test calculator
        let sum = self.calculator.add(5, 3);
        println!("5 + 3 = {}", sum);

        // Test user service
        let user = self.user_service.create_user(
            "Alice".to_string(),
            "alice@example.com".to_string()
        );
        println!("Created user: {}", user.name);
    }

    pub fn calculator(&self) -> &Calculator {
        &self.calculator
    }

    pub fn user_service(&self) -> &UserService {
        &self.user_service
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility function using imports
pub fn quick_math(x: i32, y: i32) -> i32 {
    let sum = math::add(x, y);
    math::multiply(sum, 2)
}
