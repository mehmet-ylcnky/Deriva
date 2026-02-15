# Deriva Docker Development Environment

## Quick Start

### Single Node (Phase 2 testing)
```bash
# Build image
docker build -t deriva:latest .

# Run single node
docker run -p 50051:50051 -p 9090:9090 -v $(pwd)/data:/data deriva:latest
```

### Multi-Node Cluster (Phase 3 testing)
```bash
# Start 3-node cluster
docker-compose up -d

# View logs
docker-compose logs -f

# Stop cluster
docker-compose down

# Clean volumes (fresh start)
docker-compose down -v
```

## Node Access

| Node | gRPC Port | Admin Port | Container |
|------|-----------|------------|-----------|
| node1 | 50051 | 9091 | deriva-node1 |
| node2 | 50052 | 9092 | deriva-node2 |
| node3 | 50053 | 9093 | deriva-node3 |

## Testing

### Connect to node1
```bash
# Using deriva-cli (when implemented)
deriva-cli --endpoint http://localhost:50051 status

# Using grpcurl
grpcurl -plaintext localhost:50051 list
```

### Check cluster health
```bash
# Node 1 admin API
curl http://localhost:9091/health

# Node 2 admin API
curl http://localhost:9092/health

# Node 3 admin API
curl http://localhost:9093/health
```

## Development Workflow

### Rebuild after code changes
```bash
docker-compose build
docker-compose up -d
```

### Exec into container
```bash
docker exec -it deriva-node1 /bin/bash
```

### View data directory
```bash
docker exec deriva-node1 ls -la /data
```

## Network Configuration

- Network: `deriva-net` (172.20.0.0/16)
- Nodes can communicate via hostname (node1, node2, node3)
- Seed node: node1:50051

## Volumes

- `node1-data`: Persistent storage for node1
- `node2-data`: Persistent storage for node2
- `node3-data`: Persistent storage for node3

Data survives container restarts but not `docker-compose down -v`.

## Troubleshooting

### Container won't start
```bash
docker-compose logs node1
```

### Network issues
```bash
docker network inspect deriva-net
```

### Clean slate
```bash
docker-compose down -v
docker system prune -f
docker-compose up -d
```
