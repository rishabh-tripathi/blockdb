#!/bin/bash
# BlockDB Docker Build Script

set -e

# Configuration
IMAGE_NAME=${IMAGE_NAME:-"blockdb"}
IMAGE_TAG=${IMAGE_TAG:-"latest"}
BUILD_ARGS=""
PLATFORM=""

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
        --tag|-t)
            IMAGE_TAG="$2"
            shift 2
            ;;
        --name|-n)
            IMAGE_NAME="$2"
            shift 2
            ;;
        --platform|-p)
            PLATFORM="--platform $2"
            shift 2
            ;;
        --no-cache)
            BUILD_ARGS="$BUILD_ARGS --no-cache"
            shift
            ;;
        --push)
            PUSH_IMAGE=true
            shift
            ;;
        --help|-h)
            echo "BlockDB Docker Build Script"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -t, --tag TAG       Docker image tag (default: latest)"
            echo "  -n, --name NAME     Docker image name (default: blockdb)"
            echo "  -p, --platform PLATFORM  Target platform (e.g., linux/amd64,linux/arm64)"
            echo "  --no-cache          Build without using cache"
            echo "  --push              Push image to registry after build"
            echo "  -h, --help          Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                                    # Build blockdb:latest"
            echo "  $0 -t v1.0.0                        # Build blockdb:v1.0.0"
            echo "  $0 -n myregistry/blockdb -t v1.0.0  # Build myregistry/blockdb:v1.0.0"
            echo "  $0 --platform linux/amd64,linux/arm64 --push  # Multi-arch build and push"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            echo "Use --help for usage information."
            exit 1
            ;;
    esac
done

# Validate environment
log_info "Validating build environment..."

if ! command -v docker &> /dev/null; then
    log_error "Docker is not installed or not in PATH"
    exit 1
fi

if ! docker info &> /dev/null; then
    log_error "Docker daemon is not running or not accessible"
    exit 1
fi

# Check if we're in the right directory
if [ ! -f "Dockerfile" ]; then
    log_error "Dockerfile not found. Please run this script from the BlockDB root directory."
    exit 1
fi

if [ ! -f "Cargo.toml" ]; then
    log_error "Cargo.toml not found. Please run this script from the BlockDB root directory."
    exit 1
fi

# Get version from Cargo.toml if tag is 'latest'
if [ "$IMAGE_TAG" = "latest" ]; then
    if command -v grep &> /dev/null && command -v cut &> /dev/null; then
        CARGO_VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
        if [ -n "$CARGO_VERSION" ]; then
            log_info "Found version in Cargo.toml: $CARGO_VERSION"
            # Also tag with version
            VERSION_TAG="$CARGO_VERSION"
        fi
    fi
fi

# Build the image
FULL_IMAGE_NAME="$IMAGE_NAME:$IMAGE_TAG"
log_info "Building Docker image: $FULL_IMAGE_NAME"

if [ -n "$PLATFORM" ]; then
    log_info "Building for platforms: ${PLATFORM#--platform }"
fi

# Start timer
START_TIME=$(date +%s)

# Build command
BUILD_CMD="docker build $PLATFORM $BUILD_ARGS -t $FULL_IMAGE_NAME ."

if [ -n "$VERSION_TAG" ] && [ "$VERSION_TAG" != "$IMAGE_TAG" ]; then
    BUILD_CMD="$BUILD_CMD -t $IMAGE_NAME:$VERSION_TAG"
    log_info "Also tagging as: $IMAGE_NAME:$VERSION_TAG"
fi

log_info "Running: $BUILD_CMD"
eval $BUILD_CMD

# Calculate build time
END_TIME=$(date +%s)
BUILD_TIME=$((END_TIME - START_TIME))

log_success "Docker image built successfully in ${BUILD_TIME} seconds"
log_success "Image: $FULL_IMAGE_NAME"

# Show image size
IMAGE_SIZE=$(docker images --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}" | grep "^$IMAGE_NAME:$IMAGE_TAG" | awk '{print $2}')
if [ -n "$IMAGE_SIZE" ]; then
    log_info "Image size: $IMAGE_SIZE"
fi

# List all tags for this image
log_info "Available tags for $IMAGE_NAME:"
docker images --format "table {{.Repository}}:{{.Tag}}\t{{.Size}}\t{{.CreatedAt}}" | grep "^$IMAGE_NAME:" | head -5

# Push if requested
if [ "$PUSH_IMAGE" = "true" ]; then
    log_info "Pushing image to registry..."
    docker push "$FULL_IMAGE_NAME"
    
    if [ -n "$VERSION_TAG" ] && [ "$VERSION_TAG" != "$IMAGE_TAG" ]; then
        docker push "$IMAGE_NAME:$VERSION_TAG"
    fi
    
    log_success "Image pushed successfully"
fi

# Security scan (if available)
if command -v docker &> /dev/null && docker --help | grep -q "scout"; then
    log_info "Running security scan..."
    docker scout cves "$FULL_IMAGE_NAME" 2>/dev/null || log_warning "Security scan failed or Docker Scout not available"
fi

log_success "Build completed successfully!"

# Show next steps
echo ""
log_info "Next steps:"
echo "  • Test the image:     docker run --rm $FULL_IMAGE_NAME cli --help"
echo "  • Run single node:    docker-compose up"
echo "  • Run cluster:        docker-compose -f docker-compose.cluster.yml up"
echo "  • Push to registry:   docker push $FULL_IMAGE_NAME"