package com.example.app

import com.example.core.BaseFigure
import com.example.util.Circle
import com.example.util.Formatter
import com.example.util.Point
import com.example.util.shapeArea


class Square(private val side: Double) : BaseFigure("square") {
    override fun describe(): String = "square"

    override fun area(): Double = side * side
}

fun run(): String {
    val helper = Circle(2.0)
    val point = Point(1.0, 2.0)
    val area = helper.area()
    val formatted = Formatter.format(area)
    val total = shapeArea(Square(3.0))
    return "$formatted/$total/${point.x}"
}