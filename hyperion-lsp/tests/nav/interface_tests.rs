// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive interface implementation tests
// Testing: implements, interface method navigation, default methods, multiple interfaces
lsp_test_suite! {
    name: "Interface Implementation",

    // Basic implements navigation
    test implements_clause_navigation {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService implements Servi<|>ce {
                    public void execute() {}
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    void execute();
                }
            "
        },
        nav: "com/example/UserService.java", "<|>" => "Service.java"
    }

    // @Override interface method
    test override_interface_method {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService implements Service {
                    @Override
                    public void execu<|>te() {}
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    void execute();
                }
            "
        },
        nav: "com/example/UserService.java", "<|>" => "Service.java"
    }

    // Call interface method on implementation
    test call_interface_method_on_impl {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Service service = new UserService();
                        service.execu<|>te();
                    }
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    void execute();
                }
            ",
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService implements Service {
                    public void execute() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Service.java"
    }

    // Multiple interfaces
    test multiple_interfaces {
        files: {
            "com/example/Worker.java" => r"
                package com.example;
                public class Worker implements Runnable, Closea<|>ble {
                    public void run() {}
                    public void close() {}
                }
            ",
            "com/example/Runnable.java" => r"
                package com.example;
                public interface Runnable {
                    void run();
                }
            ",
            "com/example/Closeable.java" => r"
                package com.example;
                public interface Closeable {
                    void close();
                }
            "
        },
        nav: "com/example/Worker.java", "<|>" => "Closeable.java"
    }

    // Interface extending interface
    test interface_extends_interface {
        files: {
            "com/example/AdvancedService.java" => r"
                package com.example;
                public interface AdvancedService extends Servi<|>ce {
                    void advancedExecute();
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    void execute();
                }
            "
        },
        nav: "com/example/AdvancedService.java", "<|>" => "Service.java"
    }

    // Default method in interface
    test interface_default_method {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    default void log(String message) {
                        System.out.println(message);
                    }
                    void execute();
                }
            ",
            "com/example/UserService.java" => r#"
                package com.example;
                public class UserService implements Service {
                    public void execute() {
                        lo<|>g("Executing");
                    }
                }
            "#
        },
        nav: "com/example/UserService.java", "<|>" => "Service.java"
    }

    // Interface static method
    test interface_static_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Service.crea<|>teDefault();
                    }
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    static Service createDefault() {
                        return null;
                    }
                    void execute();
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Service.java"
    }

    // Inherited interface method
    test inherited_interface_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        AdvancedService service = new AdvancedServiceImpl();
                        service.execu<|>te();
                    }
                }
            ",
            "com/example/Service.java" => r"
                package com.example;
                public interface Service {
                    void execute();
                }
            ",
            "com/example/AdvancedService.java" => r"
                package com.example;
                public interface AdvancedService extends Service {
                    void advancedExecute();
                }
            ",
            "com/example/AdvancedServiceImpl.java" => r"
                package com.example;
                public class AdvancedServiceImpl implements AdvancedService {
                    public void execute() {}
                    public void advancedExecute() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Service.java"
    }

    // Functional interface
    test functional_interface {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        Processor p = (data) -> data.toUpperCase();
                        p.proc<|>ess("hello");
                    }
                }
            "#,
            "com/example/Processor.java" => r"
                package com.example;
                @FunctionalInterface
                public interface Processor {
                    String process(String data);
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Processor.java"
    }
}
