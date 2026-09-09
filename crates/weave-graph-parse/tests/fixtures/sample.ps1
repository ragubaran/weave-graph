function Get-Greeting {
    param($Name)
    Format-Name $Name
}

function Format-Name {
    param($Name)
    return $Name
}

Get-Greeting -Name "World"
