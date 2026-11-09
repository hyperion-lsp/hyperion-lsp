package com.hyperion.example.api.service;

import com.hyperion.example.api.dto.UserDTO;

import java.util.List;
import java.util.Optional;
import java.util.UUID;

/**
 * User service interface
 */
public interface UserService {

    /**
     * Find user by ID
     */
    Optional<UserDTO> findById(UUID id);

    /**
     * Find user by username
     */
    Optional<UserDTO> findByUsername(String username);

    /**
     * Get all users
     */
    List<UserDTO> findAll();

    /**
     * Create a new user
     */
    UserDTO create(UserDTO user);

    /**
     * Update existing user
     */
    UserDTO update(UUID id, UserDTO user);

    /**
     * Delete user
     */
    void delete(UUID id);

    /**
     * Check if user exists
     */
    boolean exists(UUID id);
}
