package reply

import (
	"os"
	"path/filepath"
)

func withFileLock(path string, action func() error) error {
	if err := os.MkdirAll(filepath.Dir(path), privateDirPerm); err != nil {
		return err
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, privateFilePerm)
	if err != nil {
		return err
	}
	defer file.Close()
	_ = file.Chmod(privateFilePerm)
	if err := lockFile(file); err != nil {
		return err
	}
	defer unlockFile(file)
	return action()
}
