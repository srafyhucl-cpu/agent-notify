package reply

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	DefaultRouteTTL      = 30 * 24 * time.Hour
	initialRouteCapacity = 64
)

var (
	ErrRouteNotFound  = errors.New("reply: route not found")
	ErrRouteAmbiguous = errors.New("reply: multiple routes match quoted message")
	ErrRouteExpired   = errors.New("reply: route expired")
)

// Route binds one outbound ClawBot message to an agent conversation.
type Route struct {
	MessageID string `json:"messageID,omitempty"`
	ClientID  string `json:"clientID,omitempty"`
	BotID     string `json:"botID"`
	UserID    string `json:"userID"`
	Agent     string `json:"agent"`
	SessionID string `json:"sessionID"`
	// Title 是可选的会话显示名，仅用于引用送达确认文案。
	Title     string    `json:"title,omitempty"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
}

// RouteStore persists and resolves quoted-message targets.
type RouteStore struct {
	Path string
	TTL  time.Duration
	Now  func() time.Time
}

func NewRouteStore(path string) *RouteStore {
	if strings.TrimSpace(path) == "" {
		path = config.GetPaths().ReplyRouteFile
	}
	return &RouteStore{
		Path: path,
		TTL:  DefaultRouteTTL,
		Now:  time.Now,
	}
}

func (s *RouteStore) Record(route Route) error {
	route = route.normalized()
	if strings.TrimSpace(route.MessageID) == "" && strings.TrimSpace(route.ClientID) == "" {
		return errors.New("reply: route has no message identifier")
	}
	if strings.TrimSpace(route.BotID) == "" || strings.TrimSpace(route.UserID) == "" {
		return errors.New("reply: route is missing ClawBot account scope")
	}
	if strings.TrimSpace(route.Agent) == "" || strings.TrimSpace(route.SessionID) == "" {
		return errors.New("reply: route is missing agent session")
	}
	now := s.now()
	if route.CreatedAt.IsZero() {
		route.CreatedAt = now
	}
	if route.ExpiresAt.IsZero() {
		route.ExpiresAt = route.CreatedAt.Add(s.ttl())
	}
	if !route.ExpiresAt.After(now) {
		return ErrRouteExpired
	}
	return appendJSONLineFiltered(s.Path, route, s.compactionKeep(now))
}

func (s *RouteStore) compactionKeep(now time.Time) func([]byte) bool {
	return func(raw []byte) bool {
		var route Route
		if err := json.Unmarshal(raw, &route); err != nil {
			return false
		}
		route = route.normalized()
		return route.valid() && routeExpiration(route, s.ttl()).After(now)
	}
}

func (s *RouteStore) Find(botID, userID, messageID, clientID string) (Route, error) {
	botID = strings.TrimSpace(botID)
	userID = strings.TrimSpace(userID)
	messageID = strings.TrimSpace(messageID)
	clientID = strings.TrimSpace(clientID)
	if botID == "" || userID == "" || (messageID == "" && clientID == "") {
		return Route{}, ErrRouteNotFound
	}

	var routes []Route
	err := withFileLock(s.Path+".lock", func() error {
		loaded, loadErr := s.load()
		if loadErr == nil {
			routes = loaded
		}
		return loadErr
	})
	if err != nil {
		return Route{}, err
	}
	now := s.now()
	var match Route
	expired := false
	matchCount := 0
	matchedTargets := make(map[string]struct{}, 1)
	for _, route := range routes {
		if strings.TrimSpace(route.BotID) != botID || strings.TrimSpace(route.UserID) != userID {
			continue
		}
		if !routeMatchesIdentifier(route, messageID, clientID) {
			continue
		}
		expires := s.expiration(route)
		if !expires.After(now) {
			expired = true
			continue
		}
		route.ExpiresAt = expires
		targetKey := routeTargetKey(route)
		if _, duplicate := matchedTargets[targetKey]; duplicate {
			continue
		}
		matchedTargets[targetKey] = struct{}{}
		matchCount++
		if matchCount == 1 {
			match = route
		}
	}

	switch matchCount {
	case 0:
		if expired {
			return Route{}, ErrRouteExpired
		}
		return Route{}, ErrRouteNotFound
	case 1:
		return match, nil
	default:
		return Route{}, ErrRouteAmbiguous
	}
}

// ListActive returns unexpired routes for one exact ClawBot account scope.
func (s *RouteStore) ListActive(botID, userID string) ([]Route, error) {
	botID = strings.TrimSpace(botID)
	userID = strings.TrimSpace(userID)
	if botID == "" || userID == "" {
		return nil, errors.New("reply: route account scope is incomplete")
	}

	var routes []Route
	err := withFileLock(s.Path+".lock", func() error {
		loaded, loadErr := s.load()
		if loadErr == nil {
			routes = loaded
		}
		return loadErr
	})
	if err != nil {
		return nil, err
	}

	now := s.now()
	active := make([]Route, 0, len(routes))
	for _, route := range routes {
		if strings.TrimSpace(route.BotID) != botID || strings.TrimSpace(route.UserID) != userID {
			continue
		}
		expires := s.expiration(route)
		if !expires.After(now) {
			continue
		}
		route.ExpiresAt = expires
		active = append(active, route)
	}
	return active, nil
}

func (s *RouteStore) load() ([]Route, error) {
	file, err := os.Open(s.Path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	routes := make([]Route, 0, initialRouteCapacity)
	scanner := newJSONLScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var route Route
		if err := json.Unmarshal([]byte(line), &route); err != nil {
			continue
		}
		route = route.normalized()
		if !route.valid() {
			continue
		}
		routes = append(routes, route)
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("reply: read routes: %w", err)
	}
	return routes, nil
}

func (r Route) normalized() Route {
	r.MessageID = strings.TrimSpace(r.MessageID)
	r.ClientID = strings.TrimSpace(r.ClientID)
	r.BotID = strings.TrimSpace(r.BotID)
	r.UserID = strings.TrimSpace(r.UserID)
	r.Agent = strings.TrimSpace(r.Agent)
	r.SessionID = strings.TrimSpace(r.SessionID)
	r.Title = strings.TrimSpace(r.Title)
	return r
}

func (r Route) valid() bool {
	return (r.MessageID != "" || r.ClientID != "") &&
		r.BotID != "" &&
		r.UserID != "" &&
		r.Agent != "" &&
		r.SessionID != ""
}

func (s *RouteStore) now() time.Time {
	if s.Now != nil {
		return s.Now()
	}
	return time.Now()
}

func (s *RouteStore) ttl() time.Duration {
	if s.TTL > 0 {
		return s.TTL
	}
	return DefaultRouteTTL
}

func (s *RouteStore) expiration(route Route) time.Time {
	return routeExpiration(route, s.ttl())
}

func routeExpiration(route Route, ttl time.Duration) time.Time {
	if !route.ExpiresAt.IsZero() {
		return route.ExpiresAt
	}
	return route.CreatedAt.Add(ttl)
}

func routeMatchesIdentifier(route Route, messageID, clientID string) bool {
	if messageID != "" && strings.TrimSpace(route.MessageID) == messageID {
		return true
	}
	return clientID != "" && strings.TrimSpace(route.ClientID) == clientID
}

func routeTargetKey(route Route) string {
	return strings.TrimSpace(route.Agent) + "\x00" + strings.TrimSpace(route.SessionID)
}

func RecordRoute(route Route) error {
	err := NewRouteStore("").Record(route)
	if err != nil {
		writeReplyDiagnostic("record route: %v", err)
	}
	return err
}
