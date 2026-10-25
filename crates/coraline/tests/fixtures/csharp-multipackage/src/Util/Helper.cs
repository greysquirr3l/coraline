using Example.Core;

namespace Example.Util;

public class Circle
{
    private readonly double _radius;

    public Circle(double radius)
    {
        _radius = radius;
    }

    public double Area()
    {
        return 3.14 * _radius * _radius;
    }
}

public static class Helper
{
    public static string Format(double value)
    {
        return value.ToString();
    }
}