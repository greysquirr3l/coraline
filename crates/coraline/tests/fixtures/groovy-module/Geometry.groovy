package geometry

import java.util.List

class Circle extends Shape {
    double radius

    double area() {
        return 3.14159 * radius * radius
    }

    String describe() {
        return "circle " + area()
    }
}

interface Shape {
    double perimeter()
}
