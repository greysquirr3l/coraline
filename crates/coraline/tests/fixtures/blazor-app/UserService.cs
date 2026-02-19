namespace BlazorApp.Services;

public class User
{
    public int Id { get; set; }
    public string Name { get; set; } = string.Empty;
    public string Email { get; set; } = string.Empty;
}

public class UserService
{
    private readonly List<User> _users = new()
    {
        new User { Id = 1, Name = "Alice", Email = "alice@example.com" },
        new User { Id = 2, Name = "Bob", Email = "bob@example.com" },
    };

    public Task<List<User>> GetUsersAsync()
    {
        return Task.FromResult(_users);
    }

    public User? FindById(int id)
    {
        return _users.FirstOrDefault(u => u.Id == id);
    }
}
