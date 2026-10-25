package com.example.app;

import com.example.core.BaseFigure;
import com.example.util.Helper;

public class Main {
    public static class Square extends BaseFigure {
        private final double side;

        public Square(double side) {
            super("square");
            this.side = side;
        }

        @Override
        public String describe() {
            return "square";
        }

        @Override
        public double area() {
            return side * side;
        }
    }

    public static String run() {
        Square square = new Square(3.0);
        double area = square.area();
        return Helper.format(area);
    }
}