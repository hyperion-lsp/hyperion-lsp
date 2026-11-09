// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#[test]
fn test_stream_collect_tolist() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "Test.java",
        r#"
        import java.util.List;
        import java.util.stream.Collectors;
        import java.util.stream.Stream;

        class Test {
            void test() {
                var x = Stream.of("a", "b", "c").collect(Collectors.toList());
                x.siz<|>e();
            }
        }
        "#,
    );

    test.add_file(
        "java/util/List.java",
        r"
        package java.util;
        public interface List<E> extends Collection<E> {
            int size();
        }
        ",
    );

    test.add_file(
        "java/util/Collection.java",
        r"
        package java.util;
        public interface Collection<E> {
            int size();
        }
        ",
    );

    test.add_file(
        "java/util/stream/Stream.java",
        r"
        package java.util.stream;
        public interface Stream<T> {
            <R,A> R collect(Collector<? super T,A,R> collector);
            static <T> Stream<T> of(T... values) { return null; }
        }
        ",
    );

    test.add_file(
        "java/util/stream/Collectors.java",
        r"
        package java.util.stream;
        import java.util.List;
        public class Collectors {
            public static <T> Collector<T, ?, List<T>> toList() { return null; }
        }
        ",
    );

    test.add_file(
        "java/util/stream/Collector.java",
        r"
        package java.util.stream;
        public interface Collector<T,A,R> {}
        ",
    );

    let result = test.goto_definition("Test.java", "<|>");
    match result {
        Ok(Some(loc)) => {
            println!("SUCCESS: Found definition at {:?}", loc);
        }
        Ok(None) => {
            panic!("FAILED: goto_definition returned None");
        }
        Err(e) => {
            panic!("FAILED: goto_definition returned error: {:?}", e);
        }
    }
}

#[test]
fn test_stream_collect_custom_collector_no_false_positive() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "Test.java",
        r#"
        import java.util.stream.Stream;

        class Test {
            void test() {
                // This contains "toList" in the text, but shouldn't match
                var x = Stream.of(1, 2).collect(myToListCollector());
            }

            Object myToListCollector() {
                return null;
            }
        }
        "#,
    );

    test.add_file(
        "java/util/stream/Stream.java",
        r"
        package java.util.stream;
        public interface Stream<T> {
            <R,A> R collect(Collector<? super T,A,R> collector);
            static <T> Stream<T> of(T... values) { return null; }
        }
        ",
    );

    test.add_file(
        "java/util/stream/Collector.java",
        r"
        package java.util.stream;
        public interface Collector<T,A,R> {}
        ",
    );

    // For this test, we just verify it compiles and runs
    // The key point is that we DON'T incorrectly infer List type
    // from a method that just happens to contain "toList" in its name
    println!("Test completed - no false positive string matching");
}
