classdef Shape
end

classdef Circle < Shape
  properties
    Radius
    Name
  end
  methods
    function obj = Circle(radius)
      obj.Radius = radius;
    end
    function a = area(obj)
      a = 3.14159 * obj.Radius ^ 2;
    end
  end
  enumeration
    ROUND
    FLAT
  end
end
