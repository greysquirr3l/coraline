/**
 * TypeScript client that calls the Rust API server
 */

interface User {
  id: number;
  name: string;
}

interface ApiResponse<T> {
  data: T;
  status: number;
}

export class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  async getUsers(): Promise<User[]> {
    const response = await fetch(`${this.baseUrl}/users`);
    return response.json();
  }

  async checkHealth(): Promise<boolean> {
    const response = await fetch(`${this.baseUrl}/health`);
    const data = await response.json();
    return data.status === "ok";
  }
}

export function createClient(port: number): ApiClient {
  return new ApiClient(`http://localhost:${port}`);
}
