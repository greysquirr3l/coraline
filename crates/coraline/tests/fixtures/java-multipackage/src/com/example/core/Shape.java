package com.example.core;

public interface Shape {
    double area();
}

public abstract class BaseFigure implements Shape {
    private final String name;

    public BaseFigure(String name) {
        this.name = name;
    }

    public String label() {
        return name + "/" + describe();
    }

    public abstract String describe();

    @Override
    public double area() {
        return 0.0;
    }
}