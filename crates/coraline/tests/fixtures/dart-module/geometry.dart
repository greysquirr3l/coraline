import 'dart:math';

class Circle extends Shape {
  final double radius;

  Circle(this.radius);

  double area() => 3.14159 * radius * radius;
}

abstract class Shape {
  double area();
}

/// A shape with a name.
class NamedShape extends Shape {
  final String name;

  NamedShape(this.name);

  @override
  double area() => 1.0;
}

double total(List<Shape> shapes) {
  var sum = 0.0;
  for (var shape in shapes) {
    sum += shape.area();
  }
  return sum;
}
