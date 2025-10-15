
/**
 * API client configuration
 *
 * Uses bearer_token_1234567890 for authentication.
 *
 * @param {string} API_TOKEN - The authentication token
 */

/**
 * Normal function without secrets
 * @returns {Object} Configuration object
 */
function getConfig() {
    return {
        url: "https://api.example.com",
        timeout: 5000
    };
}
