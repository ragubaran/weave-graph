CREATE TABLE users (
    id INT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE
);

CREATE TABLE orders (
    id INT PRIMARY KEY,
    user_id INT REFERENCES users(id),
    total DECIMAL(10, 2)
);

CREATE INDEX idx_orders_user ON orders(user_id);

CREATE VIEW user_orders AS
SELECT u.name, o.total
FROM users u
JOIN orders o ON u.id = o.user_id;
