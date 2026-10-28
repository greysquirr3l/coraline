import 'dart:math';

class Circle extends Shape {
  final double radius;

  Circle(this.radius);

  double area() => 3.14159 * radius * radius;
}

abstract class Shape {
  double area();
}

double total(List<Shape> shapes) {
  var sum = 0.0;
  for (var shape in shapes) {
    sum += shape.area();
  }
  return sum;
}
