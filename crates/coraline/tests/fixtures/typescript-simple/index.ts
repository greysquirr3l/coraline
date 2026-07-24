/**
 * Main entry point
 */

import { Calculator, add, multiply } from './math';
import { UserService, User } from './user';

export class App {
    private calculator: Calculator;
    private userService: UserService;

    constructor() {
        this.calculator = new Calculator();
        this.userService = new UserService();
    }

    run(): void {
        console.log("Starting app...");
        
        // Test calculator
        const sum = this.calculator.add(5, 3);
        console.log(`5 + 3 = ${sum}`);

        // Test user service
        const user = this.userService.createUser("Alice", "alice@example.com");
        console.log(`Created user: ${user.name}`);
    }

    getCalculator(): Calculator {
        return this.calculator;
    }

    getUserService(): UserService {
        return this.userService;
    }
}

// Utility function using imports
export function quickMath(x: number, y: number): number {
    const sum = add(x, y);
    return multiply(sum, 2);
}
