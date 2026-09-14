package reply

import (
	"context"
	"os"
	"os/exec"
)

type processEnvContextKeyType struct{}

var processEnvContextKey = processEnvContextKeyType{}

type envProcessContext struct {
	context.Context
	env []string
}

func (c envProcessContext) Value(key any) any {
	if key == processEnvContextKey {
		return c.env
	}
	return c.Context.Value(key)
}

// withProcessEnv 返回携带额外环境变量的上下文。变量只作用于通过该上下文
// 启动的子进程，不修改当前进程自身的环境。
func withProcessEnv(ctx context.Context, values map[string]string) context.Context {
	if len(values) == 0 {
		return ctx
	}
	env := os.Environ()
	for key, value := range values {
		env = append(env, key+"="+value)
	}
	return envProcessContext{Context: ctx, env: env}
}

// applyProcessEnv 把上下文携带的环境变量应用到待启动命令。
func applyProcessEnv(command *exec.Cmd, ctx context.Context) {
	if command == nil || ctx == nil {
		return
	}
	env, ok := ctx.Value(processEnvContextKey).([]string)
	if !ok {
		return
	}
	command.Env = env
}
