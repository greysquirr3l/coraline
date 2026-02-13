/**
 * User management module
 */

export interface User {
    id: number;
    name: string;
    email: string;
    createdAt: Date;
}

export class UserService {
    private users: Map<number, User> = new Map();
    private nextId: number = 1;

    createUser(name: string, email: string): User {
        const user: User = {
            id: this.nextId++,
            name,
            email,
            createdAt: new Date(),
        };
        this.users.set(user.id, user);
        return user;
    }

    getUser(id: number): User | undefined {
        return this.users.get(id);
    }

    updateUser(id: number, updates: Partial<Omit<User, 'id' | 'createdAt'>>): User | undefined {
        const user = this.users.get(id);
        if (!user) {
            return undefined;
        }
        
        const updated = { ...user, ...updates };
        this.users.set(id, updated);
        return updated;
    }

    deleteUser(id: number): boolean {
        return this.users.delete(id);
    }

    getAllUsers(): User[] {
        return Array.from(this.users.values());
    }
}
