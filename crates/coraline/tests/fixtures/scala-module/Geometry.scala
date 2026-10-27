package geometry

import scala.collection.mutable

class Circle(val radius: Double) extends Shape {
  def area(): Double = 3.14159 * radius * radius
  def describe(): String = "circle"
}

trait Shape {
  def area(): Double
}

object Geometry {
  def total(shapes: List[Shape]): Double = shapes.map(_.area()).sum
  val MAX = 10
}
