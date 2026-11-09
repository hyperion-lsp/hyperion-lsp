// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive DAO/Repository pattern tests
// Testing: DAO methods, repository pattern, persistence operations
lsp_test_suite! {
    name: "DAO and Repository Patterns",

    // Basic DAO method call
    test dao_method_call {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService {
                    private UserDao userDao;

                    public void saveUser(User user) {
                        userDao.sa<|>ve(user);
                    }
                }
            ",
            "com/example/UserDao.java" => r"
                package com.example;
                public class UserDao {
                    public void save(User user) {}
                    public User findById(String id) { return null; }
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserService.java", "<|>" => "UserDao.java"
    }

    // Repository pattern
    test repository_method {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService {
                    private UserRepository userRepository;

                    public User findUser(String id) {
                        return userRepository.findBy<|>Id(id);
                    }
                }
            ",
            "com/example/UserRepository.java" => r"
                package com.example;
                public interface UserRepository {
                    User findById(String id);
                    void save(User user);
                    void delete(User user);
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserService.java", "<|>" => "UserRepository.java"
    }

    // DAO extending base DAO
    test dao_extends_base {
        files: {
            "com/example/UserDao.java" => r"
                package com.example;
                public class UserDao extends BaseD<|>AO<User> {
                    public User findByEmail(String email) { return null; }
                }
            ",
            "com/example/BaseDAO.java" => r"
                package com.example;
                public abstract class BaseDAO<T> {
                    public void save(T entity) {}
                    public T findById(String id) { return null; }
                    public void delete(T entity) {}
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserDao.java", "<|>" => "BaseDAO.java"
    }

    // Inherited DAO method
    test inherited_dao_method {
        files: {
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService {
                    private UserDao userDao;

                    public void saveUser(User user) {
                        userDao.sa<|>ve(user);
                    }
                }
            ",
            "com/example/UserDao.java" => r"
                package com.example;
                public class UserDao extends BaseDAO<User> {
                    public User findByEmail(String email) { return null; }
                }
            ",
            "com/example/BaseDAO.java" => r"
                package com.example;
                public abstract class BaseDAO<T> {
                    public void save(T entity) {}
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserService.java", "<|>" => "BaseDAO.java"
    }

    // JPA EntityManager usage
    test jpa_entity_manager {
        files: {
            "com/example/UserDao.java" => r"
                package com.example;
                import javax.persistence.EntityManager;
                public class UserDao {
                    private EntityManager em;

                    public void save(User user) {
                        em.per<|>sist(user);
                    }
                }
            ",
            "javax/persistence/EntityManager.java" => r"
                package javax.persistence;
                public interface EntityManager {
                    void persist(Object entity);
                    <T> T find(Class<T> entityClass, Object primaryKey);
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/UserDao.java", "<|>" => "EntityManager.java"
    }

    // Native query execution
    test native_query {
        files: {
            "com/example/UserDao.java" => r#"
                package com.example;
                import javax.persistence.EntityManager;
                import javax.persistence.Query;
                public class UserDao {
                    private EntityManager em;

                    public void customQuery() {
                        Query query = em.createNativeQue<|>ry("SELECT title FROM books");
                        query.getResultList();
                    }
                }
            "#,
            "javax/persistence/EntityManager.java" => r"
                package javax.persistence;
                public interface EntityManager {
                    Query createNativeQuery(String sqlString);
                }
            ",
            "javax/persistence/Query.java" => r"
                package javax.persistence;
                import java.util.List;
                public interface Query {
                    List getResultList();
                }
            "
        },
        nav: "com/example/UserDao.java", "<|>" => "EntityManager.java"
    }
}
