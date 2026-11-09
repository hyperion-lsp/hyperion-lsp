package com.hyperion.example.service.impl;

import com.hyperion.example.api.dto.UserDTO;
import com.hyperion.example.api.service.UserService;
import com.hyperion.example.core.model.User;
import lombok.RequiredArgsConstructor;
import lombok.extern.slf4j.Slf4j;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

/**
 * User service implementation with in-memory storage
 */
@Slf4j
@Service
@RequiredArgsConstructor
@Transactional
public class UserServiceImpl implements UserService {

    private final ConcurrentHashMap<UUID, User> userStore = new ConcurrentHashMap<>();

    @Override
    public Optional<UserDTO> findById(UUID id) {
        log.debug("Finding user by ID: {}", id);
        return Optional.ofNullable(userStore.get(id))
                .map(this::toDTO);
    }

    @Override
    public Optional<UserDTO> findByUsername(String username) {
        log.debug("Finding user by username: {}", username);
        return userStore.values().stream()
                .filter(user -> user.getUsername().equals(username))
                .findFirst()
                .map(this::toDTO);
    }

    @Override
    public List<UserDTO> findAll() {
        log.debug("Finding all users");
        return userStore.values().stream()
                .map(this::toDTO)
                .toList();
    }

    @Override
    public UserDTO create(UserDTO userDTO) {
        log.info("Creating new user: {}", userDTO.getUsername());

        User user = User.builder()
                .id(UUID.randomUUID())
                .username(userDTO.getUsername())
                .email(userDTO.getEmail())
                .firstName(userDTO.getFirstName())
                .lastName(userDTO.getLastName())
                .createdAt(LocalDateTime.now())
                .updatedAt(LocalDateTime.now())
                .active(true)
                .build();

        userStore.put(user.getId(), user);
        return toDTO(user);
    }

    @Override
    public UserDTO update(UUID id, UserDTO userDTO) {
        log.info("Updating user: {}", id);

        User existingUser = userStore.get(id);
        if (existingUser == null) {
            throw new RuntimeException("User not found: " + id);
        }

        existingUser.setUsername(userDTO.getUsername());
        existingUser.setEmail(userDTO.getEmail());
        existingUser.setFirstName(userDTO.getFirstName());
        existingUser.setLastName(userDTO.getLastName());
        existingUser.setUpdatedAt(LocalDateTime.now());

        return toDTO(existingUser);
    }

    @Override
    public void delete(UUID id) {
        log.info("Deleting user: {}", id);
        userStore.remove(id);
    }

    @Override
    public boolean exists(UUID id) {
        return userStore.containsKey(id);
    }

    private UserDTO toDTO(User user) {
        return UserDTO.builder()
                .id(user.getId())
                .username(user.getUsername())
                .email(user.getEmail())
                .firstName(user.getFirstName())
                .lastName(user.getLastName())
                .createdAt(user.getCreatedAt())
                .active(user.isActive())
                .build();
    }
}
