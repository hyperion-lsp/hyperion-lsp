package v;

import lombok.*;
import lombok.extern.slf4j.Slf4j;
import java.util.*;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
class Customer {
    private Long id;
    private String name;
    private boolean active;
    private List<String> tags;
    @Builder.Default private int level = 1;
}

@Value
class Money {
    long cents;
    String currency;
}

@Getter @Setter @ToString
class Account {
    private String owner;
    private double balance;
    @Setter(AccessLevel.NONE) private final String iban = "x";
}

@Slf4j
@RequiredArgsConstructor
public class LombokUse {
    private final Customer customer;

    void test() {
        Customer c = Customer.builder().id(1L).name("n").active(true).tags(new ArrayList<>()).build();
        c.getId(); c.getName(); c.isActive(); c.getTags(); c.setName("x"); c.setActive(false); c.toString(); c.equals(c); c.hashCode();
        c.getLevel();
        Customer d = new Customer();
        Customer e = new Customer(1L, "n", true, null, 2);
        Money m = new Money(1, "USD");
        m.getCents(); m.getCurrency();
        Account a = new Account();
        a.getOwner(); a.setOwner("o"); a.getBalance(); a.setBalance(1.0); a.getIban();
        log.info("hello {}", c.getName());
        log.debug("x");
        log.error("err", new RuntimeException());
        customer.getName();
        Customer.CustomerBuilder builder = Customer.builder();
        builder.id(1L).name("x");
    }
}
