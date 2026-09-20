# PowerShell completion for pagina.
# Install: dot-source from your $PROFILE, e.g. `. "$PSScriptRoot\pagina.ps1"`.

Register-ArgumentCompleter -Native -CommandName @('pagina') -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $tokens = $commandAst.ToString() -split '\s+'
    $subcommands = @('to-html', 'to-md', 'help', 'version')
    $flags = @('--gfm', '--color', '--help', '-h', '--version', '-v')
    $colorValues = @('auto', 'always', 'never')

    # Complete `--color=<value>` inline.
    if ($wordToComplete -like '--color=*') {
        $prefix = '--color='
        $val = $wordToComplete.Substring($prefix.Length)
        return $colorValues |
            Where-Object { $_ -like "$val*" } |
            ForEach-Object {
                [System.Management.Automation.CompletionResult]::new(
                    "$prefix$_", $_, 'ParameterValue', $_)
            }
    }

    # Complete the value of a bare `--color <value>`.
    if ($tokens.Count -ge 2 -and $tokens[$tokens.Count - 2] -eq '--color') {
        return $colorValues |
            Where-Object { $_ -like "$wordToComplete*" } |
            ForEach-Object {
                [System.Management.Automation.CompletionResult]::new(
                    $_, $_, 'ParameterValue', $_)
            }
    }

    $positionals = @($tokens | Where-Object { $_ -notlike '-*' })
    if ($positionals.Count -le 2) {
        # First positional: subcommands plus global flags.
        return ($subcommands + $flags) |
            Where-Object { $_ -like "$wordToComplete*" } |
            ForEach-Object {
                [System.Management.Automation.CompletionResult]::new(
                    $_, $_, 'ParameterName', $_)
            }
    }

    $sub = $positionals[1]
    if ($sub -eq 'to-html' -or $sub -eq 'to-md') {
        if ($wordToComplete -like '-*') {
            return $flags |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new(
                        $_, $_, 'ParameterName', $_)
                }
        }
        # File arguments: fall back to default file completion.
        return @()
    }

    return $flags |
        Where-Object { $_ -like "$wordToComplete*" } |
        ForEach-Object {
            [System.Management.Automation.CompletionResult]::new(
                $_, $_, 'ParameterName', $_)
        }
}
