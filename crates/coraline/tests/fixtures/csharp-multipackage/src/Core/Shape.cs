namespace Example.Core;

public interface IShape
{
    double Area();
}

public abstract class BaseFigure : IShape
{
    public string Name { get; }

    protected BaseFigure(string name)
    {
        Name = name;
    }

    public virtual double Area()
    {
        return 0.0;
    }
}