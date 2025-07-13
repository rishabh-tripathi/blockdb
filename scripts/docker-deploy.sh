#!/bin/bash
# BlockDB Docker Deployment Script

set -e

# Configuration
DEPLOYMENT_TYPE="single"
COMPOSE_FILE="docker-compose.yml"
PROJECT_NAME="blockdb"
ENVIRONMENT="development"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --type|-t)
            DEPLOYMENT_TYPE="$2"
            case "$DEPLOYMENT_TYPE" in
                single|cluster)
                    ;;
                *)
                    log_error "Invalid deployment type: $DEPLOYMENT_TYPE. Use 'single' or 'cluster'"
                    exit 1
                    ;;
            esac
            shift 2
            ;;
        --project|-p)
            PROJECT_NAME="$2"
            shift 2
            ;;
        --env|-e)
            ENVIRONMENT="$2"
            shift 2
            ;;
        --build|-b)
            BUILD_IMAGES=true
            shift
            ;;
        --pull)
            PULL_IMAGES=true
            shift
            ;;
        --down|-d)
            ACTION="down"
            shift
            ;;
        --logs|-l)
            ACTION="logs"
            shift
            ;;
        --status|-s)
            ACTION="status"
            shift
            ;;
        --help|-h)
            echo "BlockDB Docker Deployment Script"
            echo ""
            echo "Usage: $0 [OPTIONS] [ACTION]"
            echo ""
            echo "Actions:"
            echo "  up (default)        Start the deployment"
            echo "  down               Stop and remove the deployment"
            echo "  logs               Show logs from all services"
            echo "  status             Show status of all services"
            echo ""
            echo "Options:"
            echo "  -t, --type TYPE    Deployment type: single or cluster (default: single)"
            echo "  -p, --project NAME Project name for Docker Compose (default: blockdb)"
            echo "  -e, --env ENV      Environment: development, staging, production (default: development)"
            echo "  -b, --build        Build images before starting"
            echo "  --pull             Pull images before starting"
            echo "  -d, --down         Stop and remove deployment"
            echo "  -l, --logs         Show logs"
            echo "  -s, --status       Show service status"
            echo "  -h, --help         Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                                    # Start single node"
            echo "  $0 -t cluster                        # Start 3-node cluster"
            echo "  $0 -t cluster --build               # Build and start cluster"
            echo "  $0 --down                           # Stop deployment"
            echo "  $0 --logs                           # Show logs"
            echo "  $0 -t cluster -e production         # Production cluster"
            exit 0
            ;;
        *)
            if [ -z "$ACTION" ]; then
                ACTION="$1"
                case "$ACTION" in
                    up|down|logs|status|restart)
                        ;;
                    *)
                        log_error "Unknown action: $ACTION"
                        echo "Use --help for usage information."
                        exit 1
                        ;;
                esac
            else
                log_error "Unknown option: $1"
                echo "Use --help for usage information."
                exit 1
            fi
            shift
            ;;
    esac
done

# Default action
ACTION=${ACTION:-"up"}

# Set compose file based on deployment type
case "$DEPLOYMENT_TYPE" in
    single)
        COMPOSE_FILE="docker-compose.yml"
        ;;
    cluster)
        COMPOSE_FILE="docker-compose.cluster.yml"
        ;;
esac

# Validate environment
log_info "Validating deployment environment..."

if ! command -v docker &> /dev/null; then
    log_error "Docker is not installed or not in PATH"
    exit 1
fi

if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null; then
    log_error "Docker Compose is not installed or not in PATH"
    exit 1
fi

# Use docker compose or docker-compose
if docker compose version &> /dev/null; then
    DOCKER_COMPOSE="docker compose"
else
    DOCKER_COMPOSE="docker-compose"
fi

# Check if compose file exists
if [ ! -f "$COMPOSE_FILE" ]; then
    log_error "Compose file not found: $COMPOSE_FILE"
    exit 1
fi

# Common compose arguments
COMPOSE_ARGS="-f $COMPOSE_FILE -p $PROJECT_NAME"

# Environment-specific overrides
ENV_FILE="docker/.env.$ENVIRONMENT"
if [ -f "$ENV_FILE" ]; then
    COMPOSE_ARGS="$COMPOSE_ARGS --env-file $ENV_FILE"
    log_info "Using environment file: $ENV_FILE"
fi

# Execute action
case "$ACTION" in
    up)
        log_info "Starting BlockDB deployment..."
        log_info "Type: $DEPLOYMENT_TYPE"
        log_info "Environment: $ENVIRONMENT"
        log_info "Project: $PROJECT_NAME"
        
        # Build images if requested
        if [ "$BUILD_IMAGES" = "true" ]; then
            log_info "Building images..."
            $DOCKER_COMPOSE $COMPOSE_ARGS build
        fi
        
        # Pull images if requested
        if [ "$PULL_IMAGES" = "true" ]; then
            log_info "Pulling images..."
            $DOCKER_COMPOSE $COMPOSE_ARGS pull
        fi
        
        # Start services
        log_info "Starting services..."
        $DOCKER_COMPOSE $COMPOSE_ARGS up -d
        
        # Wait for services to be healthy
        log_info "Waiting for services to be ready..."
        sleep 10
        
        # Check health
        if [ "$DEPLOYMENT_TYPE" = "single" ]; then
            if curl -f http://localhost:8080/health &> /dev/null; then
                log_success "BlockDB is ready!"
                echo "  • API endpoint: http://localhost:8080"
                echo "  • Health check: http://localhost:8080/health"
            else
                log_warning "BlockDB may not be ready yet. Check logs with: $0 --logs"
            fi
        else
            log_info "Cluster deployment started. Checking node health..."
            HEALTHY_NODES=0
            for port in 8081 8082 8083; do
                if curl -f http://localhost:$port/health &> /dev/null; then
                    ((HEALTHY_NODES++))
                    log_success "Node on port $port is healthy"
                else
                    log_warning "Node on port $port is not ready yet"
                fi
            done
            
            if [ $HEALTHY_NODES -ge 2 ]; then
                log_success "Cluster is ready with $HEALTHY_NODES/$((HEALTHY_NODES + (3 - HEALTHY_NODES))) nodes!"
                echo "  • Load balancer: http://localhost:8080"
                echo "  • Node 1: http://localhost:8081"
                echo "  • Node 2: http://localhost:8082"  
                echo "  • Node 3: http://localhost:8083"
            else
                log_warning "Cluster may not be ready yet. Check logs with: $0 --logs"
            fi
        fi
        ;;
    down)
        log_info "Stopping BlockDB deployment..."
        $DOCKER_COMPOSE $COMPOSE_ARGS down
        log_success "Deployment stopped"
        ;;
    logs)
        log_info "Showing logs for BlockDB deployment..."
        $DOCKER_COMPOSE $COMPOSE_ARGS logs -f
        ;;
    status)
        log_info "BlockDB deployment status:"
        $DOCKER_COMPOSE $COMPOSE_ARGS ps
        
        # Additional health checks
        echo ""
        log_info "Health checks:"
        if [ "$DEPLOYMENT_TYPE" = "single" ]; then
            if curl -f http://localhost:8080/health &> /dev/null; then
                log_success "Single node: HEALTHY"
            else
                log_error "Single node: UNHEALTHY"
            fi
        else
            for i in 1 2 3; do
                port=$((8080 + i))
                if curl -f http://localhost:$port/health &> /dev/null; then
                    log_success "Node $i (port $port): HEALTHY"
                else
                    log_error "Node $i (port $port): UNHEALTHY"
                fi
            done
        fi
        ;;
    restart)
        log_info "Restarting BlockDB deployment..."
        $DOCKER_COMPOSE $COMPOSE_ARGS restart
        log_success "Deployment restarted"
        ;;
esac

# Show helpful commands
if [ "$ACTION" = "up" ]; then
    echo ""
    log_info "Useful commands:"
    echo "  • View logs:        $0 --logs"
    echo "  • Check status:     $0 --status"
    echo "  • Stop deployment:  $0 --down"
    echo "  • CLI access:       docker exec -it ${PROJECT_NAME}-blockdb-1 blockdb-cli --help"
    
    if [ "$DEPLOYMENT_TYPE" = "cluster" ]; then
        echo "  • Node 1 CLI:       docker exec -it ${PROJECT_NAME}-blockdb-node1-1 blockdb-cli --help"
        echo "  • Node 2 CLI:       docker exec -it ${PROJECT_NAME}-blockdb-node2-1 blockdb-cli --help"
        echo "  • Node 3 CLI:       docker exec -it ${PROJECT_NAME}-blockdb-node3-1 blockdb-cli --help"
    fi
fi