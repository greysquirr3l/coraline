namespace Example.Core;

/// <summary>A shape with an area.</summary>
public interface IShape
{
    /// <summary>The area of the shape.</summary>
    double Area();
}

/// <summary>The base class for every figure.</summary>
public abstract class BaseFigure : IShape
{
    public string Name { get; }

    protected BaseFigure(string name)
    {
        Name = name;
    }

    /// <summary>The area of the figure.</summary>
    public virtual double Area()
    {
        return 0.0;
    }
}
