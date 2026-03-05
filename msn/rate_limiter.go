package msn

import (
	"math/rand"
	"sync"
	"time"
)

// RateLimiter implements a token bucket rate limiter with random delay
type RateLimiter struct {
	mu           sync.Mutex
	tokens       float64
	maxTokens    float64
	refillRate   float64 // tokens per second
	lastRefill   time.Time
	minDelayMs   int // minimum delay in milliseconds
	maxDelayMs   int // maximum delay in milliseconds
	requestCount int64
}

// RateLimiterConfig holds rate limiter configuration
type RateLimiterConfig struct {
	RequestsPerSecond float64 // target RPS
	MinDelayMs        int     // minimum random delay
	MaxDelayMs        int     // maximum random delay
}

// NewRateLimiter creates a new rate limiter
func NewRateLimiter(config RateLimiterConfig) *RateLimiter {
	if config.RequestsPerSecond <= 0 {
		config.RequestsPerSecond = 10 // default 10 RPS
	}

	return &RateLimiter{
		tokens:     config.RequestsPerSecond, // start with full bucket
		maxTokens:  config.RequestsPerSecond,
		refillRate: config.RequestsPerSecond,
		lastRefill: time.Now(),
		minDelayMs: config.MinDelayMs,
		maxDelayMs: config.MaxDelayMs,
	}
}

// Wait blocks until a token is available and applies random delay
func (r *RateLimiter) Wait() {
	r.mu.Lock()
	defer r.mu.Unlock()

	// Refill tokens based on elapsed time
	now := time.Now()
	elapsed := now.Sub(r.lastRefill).Seconds()
	r.tokens += elapsed * r.refillRate
	if r.tokens > r.maxTokens {
		r.tokens = r.maxTokens
	}
	r.lastRefill = now

	// Wait if no tokens available
	if r.tokens < 1 {
		waitTime := time.Duration((1-r.tokens)/r.refillRate*1000) * time.Millisecond
		r.mu.Unlock()
		time.Sleep(waitTime)
		r.mu.Lock()
		r.tokens = 0
	} else {
		r.tokens--
	}

	r.requestCount++

	// Apply random delay if configured
	if r.maxDelayMs > 0 {
		delayRange := r.maxDelayMs - r.minDelayMs
		if delayRange <= 0 {
			delayRange = 1
		}
		delay := r.minDelayMs + rand.Intn(delayRange)
		r.mu.Unlock()
		time.Sleep(time.Duration(delay) * time.Millisecond)
		r.mu.Lock()
	}
}

// RequestCount returns the total number of requests made
func (r *RateLimiter) RequestCount() int64 {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.requestCount
}

// SetRPS dynamically adjusts the rate limit
func (r *RateLimiter) SetRPS(rps float64) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.maxTokens = rps
	r.refillRate = rps
}
