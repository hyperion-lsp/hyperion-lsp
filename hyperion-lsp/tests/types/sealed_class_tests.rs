// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for sealed classes and interfaces (Java 17+)
// Priority 1: High priority - increasingly used in modern Java

lsp_test_suite! {
    name: "Sealed Class Navigation",

    test navigate_to_permitted_class {
        files: {
            "com/example/Shape.java" => r"
                package com.example;

                public sealed interface Shape
                    permits Circl<|>e, Rectangle, Triangle {
                    double area();
                }
            ",
            "com/example/Circle.java" => r"
                package com.example;

                public final class Circle implements Shape {
                    public double area() { return 0; }
                }
            ",
            "com/example/Rectangle.java" => r"
                package com.example;

                public final class Rectangle implements Shape {
                    public double area() { return 0; }
                }
            ",
            "com/example/Triangle.java" => r"
                package com.example;

                public final class Triangle implements Shape {
                    public double area() { return 0; }
                }
            "
        },
        nav: "com/example/Shape.java", "<|>" => "Circle.java"
    }

    test navigate_from_permitted_to_sealed {
        files: {
            "com/example/Shape.java" => r"
                package com.example;

                public sealed interface Shape
                    permits Circle {
                    double area();
                }
            ",
            "com/example/Circle.java" => r"
                package com.example;

                public final class Circle implements Shap<|>e {
                    public double area() { return 0; }
                }
            "
        },
        nav: "com/example/Circle.java", "<|>" => "Shape.java"
    }

    test sealed_class_with_abstract_permitted {
        files: {
            "com/example/Vehicle.java" => r"
                package com.example;

                public sealed class Vehicle
                    permits Car, Tru<|>ck {
                    public abstract void drive();
                }
            ",
            "com/example/Car.java" => r"
                package com.example;

                public final class Car extends Vehicle {
                    public void drive() {}
                }
            ",
            "com/example/Truck.java" => r"
                package com.example;

                public non-sealed class Truck extends Vehicle {
                    public void drive() {}
                }
            "
        },
        nav: "com/example/Vehicle.java", "<|>" => "Truck.java"
    }

    test sealed_interface_multiple_permits {
        files: {
            "com/example/Result.java" => r"
                package com.example;

                public sealed interface Result<T>
                    permits Success, Failure, Pendin<|>g {
                }
            ",
            "com/example/Success.java" => r"
                package com.example;

                public final class Success<T> implements Result<T> {
                    private final T value;
                    public Success(T value) { this.value = value; }
                }
            ",
            "com/example/Failure.java" => r"
                package com.example;

                public final class Failure<T> implements Result<T> {
                    private final Exception error;
                    public Failure(Exception error) { this.error = error; }
                }
            ",
            "com/example/Pending.java" => r"
                package com.example;

                public final class Pending<T> implements Result<T> {
                }
            "
        },
        nav: "com/example/Result.java", "<|>" => "Pending.java"
    }

    test nested_sealed_interfaces {
        files: {
            "com/example/Renderer.java" => r"
                package com.example;

                public sealed interface Renderer
                    permits Renderer.Default, Renderer.Custom {

                    sealed interface Default extends Renderer
                        permits DefaultIm<|>pl {
                    }

                    final class DefaultImpl implements Default {
                    }

                    non-sealed interface Custom extends Renderer {
                    }
                }
            ",
            "com/example/Renderer/DefaultImpl.java" => r"
                package com.example;

                final class DefaultImpl implements Renderer.Default {
                }
            "
        },
        nav: "com/example/Renderer.java", "<|>" => "Renderer.java"
    }

    test sealed_class_same_file_permits {
        files: {
            "com/example/Expression.java" => r"
                package com.example;

                public sealed interface Expression
                    permits Constant, Addition, Multiplication {
                }

                final class Constant implements Expression {
                    private final int value;
                    public Constant(int value) { this.value = value; }
                }

                final class Addition implements Expression {
                    private final Expression left, right;
                    public Addition(Expression left, Expression right) {
                        this.left = left;
                        this.right = right;
                    }
                }

                final class Multiplication implements Expressi<|>on {
                    private final Expression left, right;
                    public Multiplication(Expression left, Expression right) {
                        this.left = left;
                        this.right = right;
                    }
                }
            "
        },
        nav: "com/example/Expression.java", "<|>" => "Expression.java"
    }

    test sealed_abstract_class {
        files: {
            "com/example/Node.java" => r"
                package com.example;

                public sealed abstract class Node
                    permits LeafNode, BranchNode {
                    public abstract String describe();
                }
            ",
            "com/example/LeafNode.java" => r#"
                package com.example;

                public final class LeafNode extends Nod<|>e {
                    public String describe() { return "leaf"; }
                }
            "#,
            "com/example/BranchNode.java" => r#"
                package com.example;

                public non-sealed class BranchNode extends Node {
                    public String describe() { return "branch"; }
                }
            "#
        },
        nav: "com/example/LeafNode.java", "<|>" => "Node.java"
    }

    test sealed_with_generics {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public sealed interface Container<T>
                    permits ListContaine<|>r, SetContainer {
                    void add(T item);
                }
            ",
            "com/example/ListContainer.java" => r"
                package com.example;
                import java.util.*;

                public final class ListContainer<T> implements Container<T> {
                    private final List<T> items = new ArrayList<>();
                    public void add(T item) { items.add(item); }
                }
            ",
            "com/example/SetContainer.java" => r"
                package com.example;
                import java.util.*;

                public final class SetContainer<T> implements Container<T> {
                    private final Set<T> items = new HashSet<>();
                    public void add(T item) { items.add(item); }
                }
            "
        },
        nav: "com/example/Container.java", "<|>" => "ListContainer.java"
    }

    test non_sealed_permits {
        files: {
            "com/example/Base.java" => r"
                package com.example;

                public sealed class Base
                    permits Middleware {
                }
            ",
            "com/example/Middleware.java" => r"
                package com.example;

                public non-sealed class Middleware extends Bas<|>e {
                    // Can be extended by any class now
                }
            ",
            "com/example/Extended.java" => r"
                package com.example;

                public class Extended extends Middleware {
                }
            "
        },
        nav: "com/example/Middleware.java", "<|>" => "Base.java"
    }

    test sealed_permits_first_class {
        files: {
            "com/example/Animal.java" => r"
                package com.example;

                public sealed interface Animal
                    permits D<|>og, Cat, Bird {
                    String makeSound();
                }
            ",
            "com/example/Dog.java" => r#"
                package com.example;

                public final class Dog implements Animal {
                    public String makeSound() { return "Woof"; }
                }
            "#,
            "com/example/Cat.java" => r#"
                package com.example;

                public final class Cat implements Animal {
                    public String makeSound() { return "Meow"; }
                }
            "#,
            "com/example/Bird.java" => r#"
                package com.example;

                public final class Bird implements Animal {
                    public String makeSound() { return "Tweet"; }
                }
            "#
        },
        nav: "com/example/Animal.java", "<|>" => "Dog.java"
    }
}
