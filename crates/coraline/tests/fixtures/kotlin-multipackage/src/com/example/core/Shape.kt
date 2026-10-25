package com.example.core

interface Shape {
    fun area(): Double
}

abstract class BaseFigure(
    val name: String,
) : Shape {
    abstract fun describe(): String

    override fun area(): Double = 0.0

    fun label(): String = "$name/${describe()}"
}