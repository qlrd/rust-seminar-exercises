# rust-seminar-exercises

Divided into two parts:

- Task 1: implement the sipa's seeder on to docker container;
- Task 2: implement a daemon node that can be used to run from a given IP.

## Task 1

```
docker-composer -f task-1/docker-compose.yml up --build
```

## Task 2

```
RUST_LOG=<debug|warn|info> cargo run --release --bin task-2
```
