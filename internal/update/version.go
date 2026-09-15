//go:build windows

package update

import (
	"errors"
	"fmt"
	"strconv"
	"strings"
)

var errNotComparable = errors.New("版本号无法比较")

func normalizeVersion(value string) (string, bool) {
	value = strings.TrimSpace(value)
	value = strings.TrimPrefix(value, "v")
	if value == "" {
		return "", false
	}
	if index := strings.IndexAny(value, "-+"); index >= 0 {
		value = value[:index]
	}
	parts := strings.Split(value, ".")
	if len(parts) != 3 {
		return "", false
	}
	for _, part := range parts {
		number, err := strconv.Atoi(part)
		if err != nil || number < 0 {
			return "", false
		}
	}
	return strings.Join(parts, "."), true
}

func isNewerVersion(candidate, current string) (bool, error) {
	candidateParts, candidateOK := versionParts(candidate)
	currentParts, currentOK := versionParts(current)
	if !candidateOK || !currentOK {
		return false, fmt.Errorf("%w：latest=%q current=%q", errNotComparable, candidate, current)
	}
	for index := range candidateParts {
		if candidateParts[index] != currentParts[index] {
			return candidateParts[index] > currentParts[index], nil
		}
	}
	return false, nil
}

func versionParts(value string) ([3]int, bool) {
	normalized, ok := normalizeVersion(value)
	if !ok {
		return [3]int{}, false
	}
	parts := strings.Split(normalized, ".")
	var result [3]int
	for index, part := range parts {
		number, err := strconv.Atoi(part)
		if err != nil {
			return [3]int{}, false
		}
		result[index] = number
	}
	return result, true
}
