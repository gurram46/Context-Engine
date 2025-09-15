# Database Schema

This project is a CLI tool and does not maintain a database. If you adapt Context Engine within a project that has a database, document key entities and relationships here.

## Example Structure (for DB-backed apps)

- Users
  - id (PK)
  - email (unique)
  - created_at

- Projects
  - id (PK)
  - name
  - owner_id (FK -> Users.id)

- Relations
  - One User owns many Projects (Users.id -> Projects.owner_id)

Replace the above with your application’s actual entities, fields, and relations.
