.PHONY: dev-up dev-down dev-logs dev-ps

# Boot the full Rust stack in the background and tail gateway health
dev-up:
	docker compose -f docker-compose.rust.yaml up --build -d
	@echo "Waiting for sera-gateway health..."
	@until docker compose -f docker-compose.rust.yaml exec -T sera-gateway curl -sf http://localhost:3001/api/health >/dev/null 2>&1; do \
		echo "  still waiting..."; \
		sleep 5; \
	done
	@echo "sera-gateway is healthy at http://localhost:3001"

dev-down:
	docker compose -f docker-compose.rust.yaml down

dev-logs:
	docker compose -f docker-compose.rust.yaml logs -f sera-gateway

dev-ps:
	docker compose -f docker-compose.rust.yaml ps
