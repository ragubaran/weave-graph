<?php

require 'helpers.php';

interface Named {
    public function getName(): string;
}

class Greeter implements Named {
    public function getName(): string {
        return $this->name;
    }

    public function greet(): string {
        return formatName($this->getName());
    }
}

class LoudGreeter extends Greeter {
}

function formatName($name) {
    return trim($name);
}
