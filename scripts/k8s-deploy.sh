#!/bin/bash
# BlockDB Kubernetes Deployment Script

set -e

# Configuration
NAMESPACE="blockdb"
KUBECTL_ARGS=""
DRY_RUN=""
ENVIRONMENT="development"
REGISTRY=""
TAG="latest"

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
        --namespace|-n)
            NAMESPACE="$2"
            shift 2
            ;;
        --dry-run|-d)
            DRY_RUN="--dry-run=client"
            shift
            ;;
        --environment|-e)
            ENVIRONMENT="$2"
            shift 2
            ;;
        --registry|-r)
            REGISTRY="$2"
            shift 2
            ;;
        --tag|-t)
            TAG="$2"
            shift 2
            ;;
        --context|-c)
            KUBECTL_ARGS="$KUBECTL_ARGS --context=$2"
            shift 2
            ;;
        --delete)
            ACTION="delete"
            shift
            ;;
        --status|-s)
            ACTION="status"
            shift
            ;;
        --logs|-l)
            ACTION="logs"
            shift
            ;;
        --port-forward|-p)
            ACTION="port-forward"
            shift
            ;;
        --help|-h)
            echo "BlockDB Kubernetes Deployment Script"
            echo ""
            echo "Usage: $0 [OPTIONS] [ACTION]"
            echo ""
            echo "Actions:"
            echo "  deploy (default)    Deploy BlockDB to Kubernetes"
            echo "  delete             Delete BlockDB deployment"
            echo "  status             Show deployment status"
            echo "  logs               Show logs from BlockDB pods"
            echo "  port-forward       Setup port forwarding for local access"
            echo ""
            echo "Options:"
            echo "  -n, --namespace NS     Kubernetes namespace (default: blockdb)"
            echo "  -e, --environment ENV  Environment: development, staging, production"
            echo "  -r, --registry REG     Docker registry for images"
            echo "  -t, --tag TAG          Image tag (default: latest)"
            echo "  -c, --context CTX      Kubernetes context"
            echo "  -d, --dry-run          Perform dry run without applying changes"
            echo "  --delete              Delete the deployment"
            echo "  -s, --status          Show deployment status"
            echo "  -l, --logs            Show logs"
            echo "  -p, --port-forward    Setup port forwarding"
            echo "  -h, --help            Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                                    # Deploy to default namespace"
            echo "  $0 -n production -e production       # Deploy to production"
            echo "  $0 -r myregistry.com -t v1.0.0      # Deploy specific image"
            echo "  $0 --dry-run                         # Preview changes"
            echo "  $0 --delete                          # Delete deployment"
            echo "  $0 --status                          # Check status"
            echo "  $0 --logs                            # View logs"
            echo "  $0 --port-forward                    # Setup port forwarding"
            exit 0
            ;;
        *)
            if [ -z "$ACTION" ]; then
                ACTION="$1"
                case "$ACTION" in
                    deploy|delete|status|logs|port-forward)
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
ACTION=${ACTION:-"deploy"}

# Validate environment
log_info "Validating Kubernetes environment..."

if ! command -v kubectl &> /dev/null; then
    log_error "kubectl is not installed or not in PATH"
    exit 1
fi

# Check kubectl connection
if ! kubectl $KUBECTL_ARGS cluster-info &> /dev/null; then
    log_error "Cannot connect to Kubernetes cluster"
    log_error "Check your kubeconfig and cluster connectivity"
    exit 1
fi

# Check if k8s directory exists
if [ ! -d "k8s" ]; then
    log_error "k8s directory not found. Please run this script from the BlockDB root directory."
    exit 1
fi

# Environment-specific configurations
case "$ENVIRONMENT" in
    development)
        REPLICAS=1
        RESOURCES_REQUESTS_CPU="100m"
        RESOURCES_REQUESTS_MEMORY="256Mi"
        RESOURCES_LIMITS_CPU="500m"
        RESOURCES_LIMITS_MEMORY="1Gi"
        ;;
    staging)
        REPLICAS=2
        RESOURCES_REQUESTS_CPU="250m"
        RESOURCES_REQUESTS_MEMORY="512Mi"
        RESOURCES_LIMITS_CPU="1000m"
        RESOURCES_LIMITS_MEMORY="2Gi"
        ;;
    production)
        REPLICAS=3
        RESOURCES_REQUESTS_CPU="500m"
        RESOURCES_REQUESTS_MEMORY="1Gi"
        RESOURCES_LIMITS_CPU="2000m"
        RESOURCES_LIMITS_MEMORY="4Gi"
        ;;
    *)
        log_warning "Unknown environment: $ENVIRONMENT. Using default values."
        REPLICAS=3
        ;;
esac

# Function to apply manifests
apply_manifests() {
    log_info "Applying Kubernetes manifests..."
    
    # Create namespace first
    kubectl $KUBECTL_ARGS apply -f k8s/namespace.yaml $DRY_RUN
    
    # Apply RBAC
    kubectl $KUBECTL_ARGS apply -f k8s/rbac.yaml $DRY_RUN
    
    # Apply ConfigMap
    kubectl $KUBECTL_ARGS apply -f k8s/configmap.yaml $DRY_RUN
    
    # Apply Services
    kubectl $KUBECTL_ARGS apply -f k8s/service.yaml $DRY_RUN
    
    # Apply StatefulSet with environment-specific patches
    if [ "$DRY_RUN" = "" ]; then
        # Patch StatefulSet for environment
        kubectl $KUBECTL_ARGS patch statefulset blockdb -n $NAMESPACE --type='merge' -p="{\"spec\":{\"replicas\":$REPLICAS}}" --dry-run=client -o yaml | kubectl $KUBECTL_ARGS apply -f -
    fi
    
    kubectl $KUBECTL_ARGS apply -f k8s/statefulset.yaml $DRY_RUN
    
    # Apply PodDisruptionBudget
    kubectl $KUBECTL_ARGS apply -f k8s/pdb.yaml $DRY_RUN
    
    # Apply Ingress (optional)
    if [ -f "k8s/ingress.yaml" ]; then
        log_info "Applying Ingress configuration..."
        kubectl $KUBECTL_ARGS apply -f k8s/ingress.yaml $DRY_RUN
    fi
}

# Function to delete manifests
delete_manifests() {
    log_info "Deleting BlockDB deployment..."
    
    # Delete in reverse order
    kubectl $KUBECTL_ARGS delete -f k8s/ingress.yaml --ignore-not-found=true
    kubectl $KUBECTL_ARGS delete -f k8s/pdb.yaml --ignore-not-found=true
    kubectl $KUBECTL_ARGS delete -f k8s/statefulset.yaml --ignore-not-found=true
    kubectl $KUBECTL_ARGS delete -f k8s/service.yaml --ignore-not-found=true
    kubectl $KUBECTL_ARGS delete -f k8s/configmap.yaml --ignore-not-found=true
    kubectl $KUBECTL_ARGS delete -f k8s/rbac.yaml --ignore-not-found=true
    
    # Delete PVCs
    log_warning "Deleting PersistentVolumeClaims (data will be lost)..."
    kubectl $KUBECTL_ARGS delete pvc -l app.kubernetes.io/name=blockdb -n $NAMESPACE --ignore-not-found=true
    
    # Delete namespace last
    kubectl $KUBECTL_ARGS delete namespace $NAMESPACE --ignore-not-found=true
}

# Function to show status
show_status() {
    log_info "BlockDB Kubernetes deployment status:"
    
    echo ""
    log_info "Namespace:"
    kubectl $KUBECTL_ARGS get namespace $NAMESPACE 2>/dev/null || log_warning "Namespace $NAMESPACE not found"
    
    echo ""
    log_info "StatefulSet:"
    kubectl $KUBECTL_ARGS get statefulset -n $NAMESPACE -l app.kubernetes.io/name=blockdb 2>/dev/null || log_warning "No StatefulSet found"
    
    echo ""
    log_info "Pods:"
    kubectl $KUBECTL_ARGS get pods -n $NAMESPACE -l app.kubernetes.io/name=blockdb 2>/dev/null || log_warning "No pods found"
    
    echo ""
    log_info "Services:"
    kubectl $KUBECTL_ARGS get services -n $NAMESPACE -l app.kubernetes.io/name=blockdb 2>/dev/null || log_warning "No services found"
    
    echo ""
    log_info "PersistentVolumeClaims:"
    kubectl $KUBECTL_ARGS get pvc -n $NAMESPACE -l app.kubernetes.io/name=blockdb 2>/dev/null || log_warning "No PVCs found"
    
    echo ""
    log_info "Ingress:"
    kubectl $KUBECTL_ARGS get ingress -n $NAMESPACE 2>/dev/null || log_warning "No ingress found"
    
    # Health checks
    echo ""
    log_info "Health checks:"
    PODS=$(kubectl $KUBECTL_ARGS get pods -n $NAMESPACE -l app.kubernetes.io/name=blockdb -o jsonpath='{.items[*].metadata.name}' 2>/dev/null)
    if [ -n "$PODS" ]; then
        for pod in $PODS; do
            STATUS=$(kubectl $KUBECTL_ARGS get pod $pod -n $NAMESPACE -o jsonpath='{.status.phase}' 2>/dev/null)
            READY=$(kubectl $KUBECTL_ARGS get pod $pod -n $NAMESPACE -o jsonpath='{.status.conditions[?(@.type=="Ready")].status}' 2>/dev/null)
            if [ "$STATUS" = "Running" ] && [ "$READY" = "True" ]; then
                log_success "Pod $pod: HEALTHY"
            else
                log_error "Pod $pod: Status=$STATUS, Ready=$READY"
            fi
        done
    fi
}

# Function to show logs
show_logs() {
    log_info "BlockDB logs (last 100 lines):"
    kubectl $KUBECTL_ARGS logs -n $NAMESPACE -l app.kubernetes.io/name=blockdb --tail=100 -f
}

# Function to setup port forwarding
setup_port_forward() {
    log_info "Setting up port forwarding..."
    
    # Check if service exists
    if ! kubectl $KUBECTL_ARGS get service blockdb -n $NAMESPACE &> /dev/null; then
        log_error "Service 'blockdb' not found in namespace '$NAMESPACE'"
        exit 1
    fi
    
    log_info "Port forwarding BlockDB service..."
    log_info "API will be available at: http://localhost:8080"
    log_info "Metrics will be available at: http://localhost:9090"
    log_info "Press Ctrl+C to stop port forwarding"
    
    kubectl $KUBECTL_ARGS port-forward service/blockdb -n $NAMESPACE 8080:8080 9090:9090
}

# Execute action
case "$ACTION" in
    deploy)
        log_info "Deploying BlockDB to Kubernetes..."
        log_info "Environment: $ENVIRONMENT"
        log_info "Namespace: $NAMESPACE"
        log_info "Replicas: $REPLICAS"
        
        if [ -n "$REGISTRY" ]; then
            log_info "Registry: $REGISTRY"
        fi
        
        if [ -n "$TAG" ]; then
            log_info "Image Tag: $TAG"
        fi
        
        if [ -n "$DRY_RUN" ]; then
            log_warning "DRY RUN MODE - No changes will be applied"
        fi
        
        apply_manifests
        
        if [ -z "$DRY_RUN" ]; then
            log_success "Deployment completed!"
            
            # Wait for rollout
            log_info "Waiting for StatefulSet rollout..."
            kubectl $KUBECTL_ARGS rollout status statefulset/blockdb -n $NAMESPACE --timeout=300s
            
            show_status
        fi
        ;;
    delete)
        log_warning "This will delete the entire BlockDB deployment and all data!"
        read -p "Are you sure? (y/N): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            delete_manifests
            log_success "BlockDB deployment deleted"
        else
            log_info "Deletion cancelled"
        fi
        ;;
    status)
        show_status
        ;;
    logs)
        show_logs
        ;;
    port-forward)
        setup_port_forward
        ;;
esac

# Show helpful commands for deploy action
if [ "$ACTION" = "deploy" ] && [ -z "$DRY_RUN" ]; then
    echo ""
    log_info "Useful commands:"
    echo "  • Check status:       $0 --status -n $NAMESPACE"
    echo "  • View logs:          $0 --logs -n $NAMESPACE"
    echo "  • Port forwarding:    $0 --port-forward -n $NAMESPACE"
    echo "  • Delete deployment:  $0 --delete -n $NAMESPACE"
    echo "  • CLI access:         kubectl exec -it blockdb-0 -n $NAMESPACE -- blockdb-cli --help"
    echo "  • Service URL:        kubectl get service blockdb -n $NAMESPACE"
fi