-- Demo data for manually testing Shellport's database client.
CREATE SCHEMA shop;

CREATE TABLE shop.customers (
  id serial PRIMARY KEY,
  name text NOT NULL,
  email text UNIQUE,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE shop.orders (
  id bigserial PRIMARY KEY,
  customer_id int NOT NULL REFERENCES shop.customers(id),
  total numeric(10,2) NOT NULL,
  paid boolean NOT NULL DEFAULT false,
  meta jsonb
);

CREATE VIEW shop.unpaid_orders AS
  SELECT o.id, c.name, o.total FROM shop.orders o JOIN shop.customers c ON c.id = o.customer_id WHERE NOT o.paid;

INSERT INTO shop.customers (name, email)
SELECT 'Customer ' || i, 'customer' || i || '@example.com' FROM generate_series(1, 50) i;

INSERT INTO shop.orders (customer_id, total, paid, meta)
SELECT 1 + (i % 50), round((random() * 500)::numeric, 2), i % 3 = 0, jsonb_build_object('channel', CASE WHEN i % 2 = 0 THEN 'web' ELSE 'store' END)
FROM generate_series(1, 2500) i;

ANALYZE;
