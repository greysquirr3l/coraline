using Example.Core;
using Example.Util;

namespace Example.App;

public class Runner
{
    public string Run()
    {
        var circle = new Circle(2.0);
        var area = circle.Area();
        return Helper.Format(area);
    }
}