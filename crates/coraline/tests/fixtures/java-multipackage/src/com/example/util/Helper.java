package com.example.util;

import com.example.core.Shape;

public class Helper {
    public static String format(double value) {
        return Double.toString(value);
    }

    public static double shapeArea(Shape shape) {
        return shape.area();
    }

    public static class Point {
        public final double x;

        public Point(double x) {
            this.x = x;
        }
    }
}