/**
 * Mathematical utility functions
 */

export function add(a: number, b: number): number {
    return a + b;
}

export function subtract(a: number, b: number): number {
    return a - b;
}

export function multiply(a: number, b: number): number {
    return a * b;
}

export function divide(a: number, b: number): number {
    if (b === 0) {
        throw new Error("Division by zero");
    }
    return a / b;
}

export class Calculator {
    private history: string[] = [];

    add(a: number, b: number): number {
        const result = add(a, b);
        this.history.push(`${a} + ${b} = ${result}`);
        return result;
    }

    getHistory(): string[] {
        return [...this.history];
    }

    clearHistory(): void {
        this.history = [];
    }
}
