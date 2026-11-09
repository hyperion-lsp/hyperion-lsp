// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Additional Method Navigation Tests",

    test method_with_multiple_parameters {
        files: {
            "com/example/Calculator.java" => r"
                package com.example;

                public class Calculator {
                    public void test() {
                        ad<|>d(1, 2, 3);
                    }

                    public int add(int a, int b, int c) {
                        return a + b + c;
                    }
                }
            "
        },
        nav: "com/example/Calculator.java", "<|>" => "Calculator.java"
    }

    test method_returning_void {
        files: {
            "com/example/Logger.java" => r#"
                package com.example;

                public class Logger {
                    public void test() {
                        lo<|>g("message");
                    }

                    public void log(String msg) {
                        System.out.println(msg);
                    }
                }
            "#
        },
        nav: "com/example/Logger.java", "<|>" => "Logger.java"
    }

    test method_with_array_parameter {
        files: {
            "com/example/ArrayProcessor.java" => r"
                package com.example;

                public class ArrayProcessor {
                    public void test() {
                        int[] arr = {1, 2, 3};
                        proce<|>ss(arr);
                    }

                    public void process(int[] array) {}
                }
            "
        },
        nav: "com/example/ArrayProcessor.java", "<|>" => "ArrayProcessor.java"
    }

    test method_in_try_catch {
        files: {
            "com/example/Handler.java" => r"
                package com.example;

                public class Handler {
                    public void test() {
                        try {
                            risk<|>y();
                        } catch (Exception e) {}
                    }

                    public void risky() throws Exception {}
                }
            "
        },
        nav: "com/example/Handler.java", "<|>" => "Handler.java"
    }

    test method_in_if_statement {
        files: {
            "com/example/Validator.java" => r"
                package com.example;

                public class Validator {
                    public void test(boolean flag) {
                        if (flag) {
                            valida<|>te();
                        }
                    }

                    private void validate() {}
                }
            "
        },
        nav: "com/example/Validator.java", "<|>" => "Validator.java"
    }

    test method_in_for_loop {
        files: {
            "com/example/Processor.java" => r"
                package com.example;

                public class Processor {
                    public void test() {
                        for (int i = 0; i < 10; i++) {
                            proce<|>ss(i);
                        }
                    }

                    private void process(int n) {}
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "Processor.java"
    }

    test method_in_while_loop {
        files: {
            "com/example/Worker.java" => r"
                package com.example;

                public class Worker {
                    public void test() {
                        while (hasMor<|>e()) {
                            process();
                        }
                    }

                    private boolean hasMore() {
                        return false;
                    }

                    private void process() {}
                }
            "
        },
        nav: "com/example/Worker.java", "<|>" => "Worker.java"
    }

    test static_method_with_class_name {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.Helper;

                public class Main {
                    public void test() {
                        Helper.assiste<|>r();
                    }
                }
            ",
            "com/example/Helper.java" => r"
                package com.example;

                public class Helper {
                    public static void assister() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Helper.java"
    }

    test method_call_on_return_value {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        getUser().getNa<|>me();
                    }

                    private User getUser() {
                        return new User();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getName() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test method_with_string_concatenation_arg {
        files: {
            "com/example/Printer.java" => r#"
                package com.example;

                public class Printer {
                    public void test() {
                        prin<|>t("Hello " + "World");
                    }

                    public void print(String msg) {}
                }
            "#
        },
        nav: "com/example/Printer.java", "<|>" => "Printer.java"
    }

    test method_with_numeric_literal_args {
        files: {
            "com/example/Math.java" => r"
                package com.example;

                public class Math {
                    public void test() {
                        ad<|>d(42, 3.14, 0xFF);
                    }

                    public double add(int a, double b, int c) {
                        return a + b + c;
                    }
                }
            "
        },
        nav: "com/example/Math.java", "<|>" => "Math.java"
    }

    test private_method_call {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        internal<|>Process();
                    }

                    private void internalProcess() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test protected_method_call {
        files: {
            "com/example/Base.java" => r"
                package com.example;

                public class Base {
                    public void test() {
                        helper<|>();
                    }

                    protected void helper() {}
                }
            "
        },
        nav: "com/example/Base.java", "<|>" => "Base.java"
    }

    test method_with_boolean_args {
        files: {
            "com/example/Config.java" => r"
                package com.example;

                public class Config {
                    public void test() {
                        setEnab<|>led(true);
                    }

                    public void setEnabled(boolean enabled) {}
                }
            "
        },
        nav: "com/example/Config.java", "<|>" => "Config.java"
    }

    test method_with_null_arg {
        files: {
            "com/example/Handler.java" => r"
                package com.example;

                public class Handler {
                    public void test() {
                        handl<|>e(null);
                    }

                    public void handle(Object obj) {}
                }
            "
        },
        nav: "com/example/Handler.java", "<|>" => "Handler.java"
    }
}

lsp_test_suite! {
    name: "Cross-File Navigation Tests",

    test method_via_imported_class {
        files: {
            "com/example/Client.java" => r"
                package com.example;
                import com.example.service.Service;

                public class Client {
                    public void test() {
                        Service service = new Service();
                        service.execu<|>te();
                    }
                }
            ",
            "com/example/service/Service.java" => r"
                package com.example.service;

                public class Service {
                    public void execute() {}
                }
            "
        },
        nav: "com/example/Client.java", "<|>" => "Service.java"
    }

    test method_on_field_initialized_inline {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    private User user = new User();

                    public void test() {
                        user.getEmai<|>l();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;

                public class User {
                    public String getEmail() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test method_on_parameter {
        files: {
            "com/example/Processor.java" => r"
                package com.example;
                import com.example.model.Data;

                public class Processor {
                    public void process(Data data) {
                        data.getValu<|>e();
                    }
                }
            ",
            "com/example/model/Data.java" => r"
                package com.example.model;

                public class Data {
                    public int getValue() {
                        return 0;
                    }
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "Data.java"
    }

    test constructor_with_arguments {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import com.example.model.Person;

                public class Main {
                    public void test() {
                        Person p = new Pers<|>on("John", 30);
                    }
                }
            "#,
            "com/example/model/Person.java" => r"
                package com.example.model;

                public class Person {
                    public Person(String name, int age) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }

    test method_chain_two_levels {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;
                import com.example.model.Profile;

                public class Main {
                    public void test(User user) {
                        user.getProfile().getNa<|>me();
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    public Profile getProfile() {
                        return null;
                    }
                }
            ",
            "com/example/model/Profile.java" => r#"
                package com.example.model;

                public class Profile {
                    public String getName() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Profile.java"
    }
}
