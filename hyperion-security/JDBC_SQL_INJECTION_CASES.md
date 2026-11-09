# JDBC SQL Injection Case Matrix

This document defines the JDBC SQL-injection contract enforced by Hyperion
Security's regression suite: cases it must detect and nearby safe cases it must
not flag.

## Threat Model

We report `VulnType::SqlInjection` when attacker-controlled (or otherwise tainted) data influences SQL
code executed by the application (JDBC or JDBC-like APIs).

## Taint Sources (inputs)

Examples of inputs that should taint strings used in SQL:

- HTTP inputs: `HttpServletRequest.getParameter(..)`, `@RequestParam`, `@PathVariable`, etc.
- Other configured sources: message brokers, env, DB results, etc. (see `SourceRegistry`).

## String Construction / Propagation (builders)

These should propagate taint into the resulting SQL string:

- Binary concatenation: `"..." + userInput` (AST: `BINARY_EXPR`)
- `StringBuilder.append(..)` / `.toString()`
- `String.format(..)` / `String.join(..)`
- Variable/field assignments and reassignments of the above

## JDBC Sink Cases (what must be detected)

### 1) `Statement.execute*(String sql)` with tainted SQL

Examples:

```java
Statement st = conn.createStatement();
st.executeQuery("SELECT ... WHERE id = " + whoops);
st.executeUpdate(query);
st.execute(sql);
st.addBatch(sql);
```

Expected: finding at the call site (sink), with taint flow.

### 2) `Connection.prepareStatement(String sql)` / `prepareCall(String sql)` with tainted SQL

Examples:

```java
PreparedStatement ps = conn.prepareStatement("SELECT ... WHERE id = " + whoops);
CallableStatement cs = conn.prepareCall(sql);
conn.nativeSQL(sql);
```

Expected: finding at the prepare* call site.

### 3) `PreparedStatement` misuse: inherited `Statement.execute*(String sql)` overloads

`PreparedStatement` extends `Statement`, so these overloads exist and are vulnerable:

```java
PreparedStatement ps = conn.prepareStatement("SELECT ... WHERE id = ?");
ps.executeQuery("SELECT ... WHERE id = " + whoops); // misuse
```

Expected: finding at the execute* call site (same as `Statement`).

### 4) Chained JDBC calls (no intermediate variables)

Examples:

```java
conn.createStatement().executeQuery("SELECT ... WHERE id = " + whoops);
dataSource.getConnection().prepareStatement("SELECT ... WHERE id = " + whoops);
dataSource.getConnection().createStatement().execute(sql);
```

Expected: finding at the sink call site (or the prepare* call site), even with chaining.

### 5) Try-with-resources resource initializers

Resource initializers must be scanned (they are not in the try-body block):

```java
try (Connection c = dataSource.getConnection();
     PreparedStatement ps = c.prepareStatement("SELECT ... WHERE id = " + whoops)) {
  ps.executeQuery();
}
```

Expected: finding at `prepareStatement(..)` (tainted SQL) and/or at `ps.executeQuery()` (tainted receiver).

### 6) Field receivers / `this.*` receivers

Examples:

```java
this.conn.prepareStatement("SELECT ... WHERE id = " + whoops);
this.statement.executeQuery(sql);
```

Expected: finding at the sink call site.

### 7) `PreparedStatement.execute*()` (no-arg) with tainted SQL template (receiver taint)

If a `PreparedStatement` (or `CallableStatement`) is created from tainted SQL, then the no-arg execute
methods must still be reported because the dangerous data is carried in the receiver:

```java
PreparedStatement ps = conn.prepareStatement("SELECT ... WHERE id = " + whoops);
ps.executeQuery();

// chaining
conn.prepareStatement("SELECT ... WHERE id = " + whoops).executeQuery();

// Java `var` should also work
var stmt = conn.prepareStatement("SELECT ... WHERE id = " + whoops);
stmt.executeQuery();
```

Expected: finding at the execute* call site (sink), with taint flow.

## JDBC Non-Findings (what must NOT be flagged)

### A) Proper parameterized `PreparedStatement` usage

```java
PreparedStatement ps = conn.prepareStatement("SELECT ... WHERE id = ?");
ps.setString(1, whoops);
ps.executeQuery();
```

Expected: no `SqlInjection` finding.

### B) Numeric conversion used in SQL string construction (false-positive reduction)

```java
int id = Integer.parseInt(request.getParameter("id"));
stmt.executeQuery("SELECT ... WHERE id = " + id);
```

Expected: no `SqlInjection` finding (query portion is numeric).

## Regression coverage

The contract is split into focused integration targets:

- `sql_injection_false_negative_repros.rs` and
  `sql_injection_concat_test.rs` cover direct statement and connection sinks;
- `try_with_resources_tests.rs` covers resource initializers;
- `prepared_statement_misuse_tests.rs` covers inherited string-taking
  `Statement` overloads;
- `jdbc_chained_calls_tests.rs` covers chained calls and field receivers;
- `prepared_statement_receiver_taint_tests.rs` covers no-argument execution,
  chaining and `var`; and
- `framework_registry_core_patterns_tests.rs` proves framework selection does
  not discard core JDBC or command-execution sinks.

Add new regressions to the narrowest target above and preserve both the finding
and non-finding sides of the contract.
