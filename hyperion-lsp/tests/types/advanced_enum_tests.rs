// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for advanced enum features

lsp_test_suite! {
    name: "Enum Constructor Arguments",

    test enum_with_constructor_args {
        files: {
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED(255, 0, 0),
                    GREEN(0, 255, 0),
                    BLUE(0, 0, 255);

                    private final int r;
                    private final int g;
                    private final int b;

                    Color(int r, int g, int b) {
                        this.r = r;
                        this.g = g;
                        this.b = b;
                    }

                    public int getRed() { return r; }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.R<|>ED;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    test enum_constructor_navigation {
        files: {
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED(255, 0, 0),
                    GREEN(0, 255, 0),
                    BLUE(0, 0, 255);

                    private final int r;
                    private final int g;
                    private final int b;

                    Color(int r, int g, int b) {
                        this.r = r;
                        this.g = g;
                        this.b = b;
                    }

                    public int get<|>Red() { return r; }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        int red = Color.RED.getRed();
                    }
                }
            "
        },
        nav: "com/example/Color.java", "<|>" => "Color.java"
    }

    test enum_method_on_constant_with_args {
        files: {
            "com/example/Color.java" => r"
                package com.example;

                public enum Color {
                    RED(255, 0, 0),
                    GREEN(0, 255, 0),
                    BLUE(0, 0, 255);

                    private final int r;
                    private final int g;
                    private final int b;

                    Color(int r, int g, int b) {
                        this.r = r;
                        this.g = g;
                        this.b = b;
                    }

                    public int getRed() { return r; }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        int red = Color.RED.get<|>Red();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }
}

lsp_test_suite! {
    name: "Enum Constant-Specific Class Bodies",

    test enum_with_constant_body {
        files: {
            "com/example/Operation.java" => r"
                package com.example;

                public enum Operation {
                    PLUS {
                        @Override
                        public double apply(double x, double y) {
                            return x + y;
                        }
                    },
                    MINUS {
                        @Override
                        public double apply(double x, double y) {
                            return x - y;
                        }
                    };

                    public abstract double apply(double x, double y);
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Operation op = Operation.P<|>LUS;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Operation.java"
    }

    test enum_constant_body_method_navigation {
        files: {
            "com/example/Operation.java" => r"
                package com.example;

                public enum Operation {
                    PLUS {
                        @Override
                        public double apply(double x, double y) {
                            return x + y;
                        }
                    },
                    MINUS {
                        @Override
                        public double apply(double x, double y) {
                            return x - y;
                        }
                    };

                    public abstract double apply(double x, double y);
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        double result = Operation.PLUS.app<|>ly(1, 2);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Operation.java"
    }

    test enum_constant_args_and_body {
        files: {
            "com/example/Planet.java" => r"
                package com.example;

                public enum Planet {
                    EARTH(5.976e+24, 6.37814e6) {
                        @Override
                        public double surfaceWeight(double mass) {
                            return mass * surfaceGravity();
                        }
                    },
                    MARS(6.421e+23, 3.3972e6) {
                        @Override
                        public double surfaceWeight(double mass) {
                            return mass * surfaceGravity();
                        }
                    };

                    private final double mass;
                    private final double radius;

                    Planet(double mass, double radius) {
                        this.mass = mass;
                        this.radius = radius;
                    }

                    private double surfaceGravity() {
                        final double G = 6.67300E-11;
                        return G * mass / (radius * radius);
                    }

                    public abstract double surfaceWeight(double mass);
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        double weight = Planet.EARTH.surface<|>Weight(100);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Planet.java"
    }
}

lsp_test_suite! {
    name: "Enum Implementing Interfaces",

    test enum_implements_interface {
        files: {
            "com/example/Printable.java" => r"
                package com.example;

                public interface Printable {
                    void print();
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color implements Printable {
                    RED, GREEN, BLUE;

                    @Override
                    public void print() {
                        System.out.println(this.name());
                    }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color color = Color.R<|>ED;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }

    test enum_interface_method_navigation {
        files: {
            "com/example/Printable.java" => r"
                package com.example;

                public interface Printable {
                    void print();
                }
            ",
            "com/example/Color.java" => r"
                package com.example;

                public enum Color implements Printable {
                    RED, GREEN, BLUE;

                    @Override
                    public void print() {
                        System.out.println(this.name());
                    }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Color.RED.pri<|>nt();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Color.java"
    }
}
