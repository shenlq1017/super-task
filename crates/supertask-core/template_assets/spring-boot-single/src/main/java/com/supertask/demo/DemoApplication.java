package com.supertask.demo;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

/**
 * SuperTask 官方模板启动类：最小 Spring Boot Web 应用。
 */
@SpringBootApplication
@RestController
public class DemoApplication {

    public static void main(String[] args) {
        SpringApplication.run(DemoApplication.class, args);
    }

    /** 根路径返回一段文本，便于启动后直接验证。 */
    @GetMapping("/")
    public String index() {
        return "SuperTask demo backend is running";
    }
}
